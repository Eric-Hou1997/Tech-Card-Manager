//! A service-owned reader. Manual work retains the shared worker; this owner
//! cannot execute it, and manual workers cannot execute a service-owned task.
use crate::{store::Store, *};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct IncrementalSettings {
    pub revision: u32,
    pub interval_seconds: u32,
}
impl Default for IncrementalSettings {
    fn default() -> Self {
        Self {
            revision: 0,
            interval_seconds: 60,
        }
    }
}
impl IncrementalSettings {
    pub fn validate(&self) -> Result<()> {
        if !(30..=86400).contains(&self.interval_seconds) {
            return Err(AppError::new(
                "invalid-check-interval",
                "The background interval must be between 30 and 86400 seconds",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IncrementalStatus {
    pub phase: String,
    pub session: Option<String>,
    pub last_check: Option<String>,
    pub completed_cycles: u32,
    pub last_errors: u32,
    pub error: Option<AppError>,
}
impl Default for IncrementalStatus {
    fn default() -> Self {
        Self {
            phase: "stopped".into(),
            session: None,
            last_check: None,
            completed_cycles: 0,
            last_errors: 0,
            error: None,
        }
    }
}
pub struct IncrementalWorker {
    store: Arc<Store>,
    session: String,
    stop: Arc<AtomicBool>,
    refresh_requested: Arc<AtomicBool>,
    discovery: Arc<Mutex<Option<DiscoveryRequest>>>,
    state: Arc<Mutex<IncrementalStatus>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
type DiscoveryResult = Result<Vec<crate::emby_libraries::DiscoveredLibrary>>;
struct DiscoveryRequest {
    id: String,
    data: String,
    reply: std::sync::mpsc::Sender<DiscoveryResult>,
}
impl IncrementalWorker {
    pub fn start(
        store: Arc<Store>,
        session: &str,
        active: impl Fn() -> Result<bool> + Send + Sync + 'static,
        progress: impl Fn(&Task) + Send + 'static,
    ) -> Result<Self> {
        Self::start_with_completion(store, session, active, progress, |_| {})
    }
    /// Notify the owner after scan cleanup, outside the status lock. The callback
    /// must not join this reader; explicit stop joins it from the owning thread.
    pub fn start_with_completion(
        store: Arc<Store>,
        session: &str,
        active: impl Fn() -> Result<bool> + Send + Sync + 'static,
        progress: impl Fn(&Task) + Send + 'static,
        completed: impl FnOnce(IncrementalStatus) + Send + 'static,
    ) -> Result<Self> {
        // Validate the service identity through a settings-independent contract.
        if session.is_empty()
            || session.len() > 96
            || !session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(AppError::new(
                "invalid-operation-id",
                "Invalid incremental service identity",
            ));
        }
        store.incremental_settings()?.validate()?;
        let stop = Arc::new(AtomicBool::new(false));
        let refresh_requested = Arc::new(AtomicBool::new(false));
        let refresh = refresh_requested.clone();
        let discovery: Arc<Mutex<Option<DiscoveryRequest>>> = Arc::new(Mutex::new(None));
        let requested_discovery = discovery.clone();
        let state = Arc::new(Mutex::new(IncrementalStatus {
            phase: "waiting".into(),
            session: Some(session.into()),
            ..Default::default()
        }));
        let shared = state.clone();
        let stopping = stop.clone();
        let db = store.clone();
        let identity = session.to_owned();
        let thread = std::thread::Builder::new()
            .name("service-incremental".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let mut last: Option<Instant> = None;
                    let mut cycle = 0u32;
                    'reader: while !stopping.load(Ordering::SeqCst) {
                        if !active()? {
                            break;
                        }
                        let request = requested_discovery
                            .lock()
                            .map_err(|e| AppError::new("incremental-state", e))?
                            .take();
                        if let Some(request) = request {
                            let mut result =
                                db.discover_libraries_cancellable(&request.data, stopping.clone());
                            if let Err(error) = db.finish_discovery_job(&request.id, &result) {
                                result = Err(error);
                            }
                            let mut status = shared
                                .lock()
                                .map_err(|e| AppError::new("incremental-state", e))?;
                            status.error = result.as_ref().err().cloned();
                            status.phase = "waiting".into();
                            let _ = request.reply.send(result);
                            continue;
                        }
                        let rebuild = db.has_rebuild_work(&identity)?;
                        if (rebuild || db.has_manager_scan(&identity)?) && !db.has_manual_work()? {
                            shared
                                .lock()
                                .map_err(|error| AppError::new("incremental-state", error))?
                                .phase = "checking".into();
                            let probe_error = std::cell::RefCell::new(None);
                            let cancelled = || {
                                if stopping.load(Ordering::SeqCst) {
                                    return true;
                                }
                                match active() {
                                    Ok(active) => !active,
                                    Err(error) => {
                                        *probe_error.borrow_mut() = Some(error);
                                        true
                                    }
                                }
                            };
                            let outcome = if rebuild {
                                db.run_rebuild(&identity, cancelled, &progress)
                            } else {
                                db.run_service_scan(&identity, cancelled, &progress)
                            };
                            if let Some(error) = probe_error.into_inner() {
                                return Err(error);
                            }
                            match outcome {
                                Ok(Some(task)) => {
                                    let mut value = shared.lock().map_err(|error| {
                                        AppError::new("incremental-state", error)
                                    })?;
                                    value.last_errors = task.errors;
                                    value.last_check = Some(crate::emby::timestamp());
                                    value.phase = "waiting".into();
                                }
                                Ok(None) => {}
                                Err(error) if error.code == "worker-busy" => {
                                    std::thread::sleep(Duration::from_millis(100))
                                }
                                Err(error) => return Err(error),
                            }
                            continue;
                        }
                        let interval = db.incremental_settings()?.interval_seconds;
                        if (!refresh.load(Ordering::SeqCst)
                            && last.is_some_and(|at| {
                                at.elapsed() < Duration::from_secs(u64::from(interval))
                            }))
                            || db.has_manual_work()?
                        {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        let explicit = {
                            let mut status = shared
                                .lock()
                                .map_err(|e| AppError::new("incremental-state", e))?;
                            if requested_discovery
                                .lock()
                                .map_err(|e| AppError::new("incremental-state", e))?
                                .is_some()
                            {
                                continue;
                            }
                            // A UI request can arrive after the loop's queue check.
                            // Its transaction holds this same state lock: check again
                            // before submitting any automatic all-library work.
                            if db.has_manager_scan(&identity)? || db.has_rebuild_work(&identity)? {
                                status.phase = "waiting".into();
                                continue;
                            }
                            status.phase = "checking".into();
                            refresh.swap(false, Ordering::SeqCst)
                        };
                        cycle = cycle.checked_add(1).ok_or_else(|| {
                            AppError::new(
                                "incremental-overflow",
                                "Incremental cycle counter exhausted",
                            )
                        })?;
                        let mut errors = 0u32;
                        if stopping.load(Ordering::SeqCst) || !active()? {
                            break;
                        }
                        let tasks = match db.submit_service_cycle(&identity, cycle, explicit) {
                            Ok(tasks) => tasks,
                            Err(error) if error.code == "task-busy" => {
                                if explicit {
                                    refresh.store(true, Ordering::SeqCst);
                                }
                                shared
                                    .lock()
                                    .map_err(|e| AppError::new("incremental-state", e))?
                                    .phase = "waiting".into();
                                continue 'reader;
                            }
                            Err(error) => return Err(error),
                        };
                        for expected in tasks {
                            loop {
                                if stopping.load(Ordering::SeqCst) || !active()? {
                                    break;
                                }
                                let checked = std::cell::Cell::new(Instant::now());
                                let became_inactive = std::cell::Cell::new(false);
                                let probe_error = std::cell::RefCell::new(None);
                                let outcome = db.run_service_scan(
                                    &identity,
                                    || {
                                        if stopping.load(Ordering::SeqCst) || became_inactive.get()
                                        {
                                            return true;
                                        }
                                        if checked.get().elapsed() >= Duration::from_millis(250) {
                                            checked.set(Instant::now());
                                            match active() {
                                                Ok(true) => {}
                                                Ok(false) => became_inactive.set(true),
                                                Err(error) => {
                                                    *probe_error.borrow_mut() = Some(error);
                                                    became_inactive.set(true);
                                                }
                                            }
                                        }
                                        became_inactive.get()
                                    },
                                    &progress,
                                );
                                if let Some(error) = probe_error.into_inner() {
                                    return Err(error);
                                }
                                match outcome {
                                    Ok(Some(task)) => {
                                        if task.id != expected.id {
                                            return Err(AppError::new(
                                                "incremental-task-mismatch",
                                                "Service cycle ran an unexpected task",
                                            ));
                                        }
                                        if !matches!(
                                            task.state,
                                            TaskState::Completed | TaskState::Failed
                                        ) {
                                            // A cancelled/paused child cannot complete this cycle.
                                            db.cancel_service_scans(&identity)?;
                                            continue 'reader;
                                        }
                                        errors = errors.saturating_add(task.errors);
                                        break;
                                    }
                                    Ok(None) => {
                                        return Err(AppError::new(
                                            "incremental-task-missing",
                                            "Service scan has no runnable task",
                                        ))
                                    }
                                    Err(error) if error.code == "worker-busy" => {
                                        std::thread::sleep(Duration::from_millis(100))
                                    }
                                    Err(error) => return Err(error),
                                }
                            }
                        }
                        if stopping.load(Ordering::SeqCst) || !active()? {
                            break;
                        }
                        last = Some(Instant::now());
                        let mut value = shared
                            .lock()
                            .map_err(|e| AppError::new("incremental-state", e))?;
                        value.phase = "waiting".into();
                        value.last_check = Some(crate::emby::timestamp());
                        value.completed_cycles += 1;
                        value.last_errors = errors;
                    }
                    Ok(())
                })();
                let cleanup = db.cancel_service_scans(&identity);
                if let Ok(mut request) = requested_discovery.lock() {
                    if let Some(request) = request.take() {
                        let mut result = Err(AppError::new("discovery-cancelled", "任务已取消"));
                        if let Err(error) = db.finish_discovery_job(&request.id, &result) {
                            result = Err(error);
                        }
                        let _ = request.reply.send(result);
                    }
                }
                let terminal = {
                    let mut value = shared.lock().unwrap_or_else(|error| error.into_inner());
                    value.error = result.err().or_else(|| cleanup.err());
                    value.phase = if value.error.is_some() {
                        "failed"
                    } else {
                        "stopped"
                    }
                    .into();
                    value.clone()
                };
                completed(terminal);
            })
            .map_err(|e| AppError::new("incremental-worker", e))?;
        Ok(Self {
            store,
            session: session.into(),
            stop,
            refresh_requested,
            discovery,
            state,
            thread: Some(thread),
        })
    }
    pub fn status(&self) -> Result<IncrementalStatus> {
        self.state
            .lock()
            .map(|v| v.clone())
            .map_err(|e| AppError::new("incremental-state", e))
    }
    /// Queue the original discovery action on the service's one reader. The
    /// caller releases its desktop lock before waiting for the reply, so stop
    /// can cancel and join the reader even while SQLite is being queried.
    pub fn request_discovery(
        &self,
        data: String,
    ) -> Result<std::sync::mpsc::Receiver<DiscoveryResult>> {
        let mut status = self
            .state
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        if self.stop.load(Ordering::SeqCst)
            || !matches!(status.phase.as_str(), "waiting" | "checking")
        {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        if status.phase == "checking"
            || self.refresh_requested.load(Ordering::SeqCst)
            || self.store.has_active_checks()?
        {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        let (reply, receiver) = std::sync::mpsc::channel();
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!(
            "discovery-{}-{}",
            self.session,
            SERIAL.fetch_add(1, Ordering::SeqCst)
        );
        self.store.begin_discovery_job(&id)?;
        *self
            .discovery
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))? =
            Some(DiscoveryRequest { id, data, reply });
        status.phase = "checking".into();
        status.error = None;
        Ok(receiver)
    }
    pub fn request_rebuild(&self, id: &str, revision: u32) -> Result<Vec<Task>> {
        let status = self
            .state
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        if self.stop.load(Ordering::SeqCst)
            || !matches!(status.phase.as_str(), "waiting" | "checking")
        {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        if self
            .discovery
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?
            .is_some()
            || status.phase == "checking"
            || self.refresh_requested.load(Ordering::SeqCst)
        {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        self.store.submit_rebuild(id, revision, &self.session)
    }
    /// Original explicit "run" after saving folders: wake this service's
    /// reader without force-reparsing, restarting it, or accepting a parallel job.
    pub fn request_refresh(&self, revision: u32) -> Result<IncrementalStatus> {
        if self.store.configuration()?.revision != revision {
            return Err(AppError::new(
                "configuration-conflict",
                "媒体目录设置已改变，请重新读取后再刷新",
            ));
        }
        let status = self
            .state
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        if self.store.has_active_checks()? {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        if status.phase == "checking" {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        if status.phase != "waiting" || self.stop.load(Ordering::SeqCst) {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        self.refresh_requested.store(true, Ordering::SeqCst);
        Ok(status.clone())
    }
    pub fn request_manager_scan(
        &self,
        id: &str,
        revision: u32,
        scope: crate::folders::ManagerScanScope,
    ) -> Result<Vec<Task>> {
        let status = self
            .state
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        if self.stop.load(Ordering::SeqCst)
            || !matches!(status.phase.as_str(), "waiting" | "checking")
        {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        if status.phase == "checking" || self.refresh_requested.load(Ordering::SeqCst) {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        self.store
            .submit_manager_scan(id, revision, &self.session, scope)
    }
    pub fn stop(&mut self) -> Result<IncrementalStatus> {
        self.stop_with_timeout(Duration::from_secs(8))
    }
    fn stop_with_timeout(&mut self, timeout: Duration) -> Result<IncrementalStatus> {
        self.stop.store(true, Ordering::SeqCst);
        self.refresh_requested.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.as_ref() {
            let deadline = Instant::now() + timeout;
            while !thread.is_finished() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(AppError::new(
                        "incremental-stop-timeout",
                        "扫描停止等待超过 8 秒，请稍后重试退出",
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
        }
        let joined = self
            .thread
            .take()
            .map(|thread| thread.join())
            .transpose()
            .map_err(|_| AppError::new("incremental-worker", "Incremental worker panicked"));
        // A previous stop may already have joined the reader but failed while
        // persisting cancellation. Retry that idempotent cleanup on every stop
        // so the owner's visible "retry exit" path cannot silently skip it.
        let cleanup = self.store.cancel_service_scans(&self.session);
        joined?;
        cleanup?;
        self.status()
    }
}
impl Drop for IncrementalWorker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("incremental-stop: {error}");
            // A timed-out explicit stop keeps this owner alive for retry. If the
            // owner itself is being destroyed, join synchronously rather than
            // detaching an active service reader with access to the store.
            if error.code == "incremental-stop-timeout" {
                if let Some(thread) = self.thread.take() {
                    if thread.join().is_err() {
                        eprintln!("incremental-stop: Incremental worker panicked");
                    }
                }
                if let Err(error) = self.store.cancel_service_scans(&self.session) {
                    eprintln!("incremental-stop: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::sync::Condvar;
    struct ReleaseGate(Arc<(Mutex<(bool, bool)>, Condvar)>);
    impl Drop for ReleaseGate {
        fn drop(&mut self) {
            if let Ok(mut state) = self.0 .0.lock() {
                state.0 = false;
            }
            self.0 .1.notify_all();
        }
    }
    fn wait(mut ready: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(Instant::now() < until, "discovery worker timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn timed_out_stop_retains_the_reader_for_a_retry() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&temp.path().join("manager.sqlite")).unwrap());
        let gate = Arc::new((Mutex::new((true, false)), Condvar::new()));
        let probe = gate.clone();
        let mut worker = IncrementalWorker::start(
            store,
            "stop-timeout",
            move || {
                let (lock, changed) = &*probe;
                let mut state = lock.lock().unwrap();
                state.1 = true;
                changed.notify_all();
                while state.0 {
                    state = changed.wait(state).unwrap();
                }
                Ok(true)
            },
            |_| {},
        )
        .unwrap();
        let _release_on_failure = ReleaseGate(gate.clone());
        wait(|| gate.0.lock().unwrap().1);

        let error = worker
            .stop_with_timeout(Duration::from_millis(20))
            .unwrap_err();
        assert_eq!(error.code, "incremental-stop-timeout");
        assert!(
            worker.thread.is_some(),
            "the timed-out reader lost its owner"
        );

        gate.0.lock().unwrap().0 = false;
        gate.1.notify_all();
        assert_eq!(
            worker
                .stop_with_timeout(Duration::from_secs(2))
                .unwrap()
                .phase,
            "stopped"
        );
        assert!(worker.thread.is_none());
    }
    #[test]
    fn one_service_reader_owns_discovery_and_stop_cancels_queued_work() {
        for cancel in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            std::fs::create_dir(root.join("data")).unwrap();
            let db = rusqlite::Connection::open(root.join("data/library.db")).unwrap();
            db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(20,2,'TV','Z:\\TV',3,NULL,NULL),(21,20,'Series','Z:\\TV\\show',6,NULL,NULL);").unwrap();
            drop(db);
            let store = Arc::new(Store::open(&root.join("manager.sqlite")).unwrap());
            // Hold the real service-health callback so request/stop ordering is
            // deterministic, rather than racing sleeps against a tiny query.
            let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
            let probe = gate.clone();
            let mut worker = IncrementalWorker::start(
                store.clone(),
                "discover-test",
                move || {
                    let (lock, changed) = &*probe;
                    let mut state = lock.lock().unwrap();
                    if state.0 {
                        state.1 = true;
                        changed.notify_all();
                        while state.0 {
                            state = changed.wait(state).unwrap();
                        }
                    }
                    Ok(true)
                },
                |_| {},
            )
            .unwrap();
            let _release_on_failure = ReleaseGate(gate.clone());
            wait(|| worker.status().unwrap().completed_cycles == 1);
            gate.0.lock().unwrap().0 = true;
            wait(|| gate.0.lock().unwrap().1);
            let data = root.to_str().unwrap().to_string();
            let reply = worker.request_discovery(data.clone()).unwrap();
            assert_eq!(worker.status().unwrap().phase, "checking");
            let job = store.manager_job().unwrap().unwrap();
            assert_eq!(job["running"], true);
            assert_eq!(job["action"], "discover-roots");
            assert!(job["log"]
                .as_str()
                .unwrap()
                .contains("原则：按 library.db 真实父子关系"));
            assert_eq!(
                worker.request_discovery(data.clone()).unwrap_err().code,
                "task-busy"
            );
            assert_eq!(worker.request_refresh(0).unwrap_err().code, "task-busy");
            assert_eq!(
                worker.request_rebuild("rebuild", 0).unwrap_err().code,
                "task-busy"
            );
            assert_eq!(
                worker
                    .request_manager_scan(
                        "scan",
                        0,
                        crate::folders::ManagerScanScope::Space(Space::Movie)
                    )
                    .unwrap_err()
                    .code,
                "task-busy"
            );
            if cancel {
                let stopping = worker.stop.clone();
                let stopped = std::thread::spawn(move || worker.stop());
                wait(|| stopping.load(Ordering::SeqCst));
                gate.0.lock().unwrap().0 = false;
                gate.1.notify_all();
                assert_eq!(stopped.join().unwrap().unwrap().phase, "stopped");
                assert_eq!(
                    reply
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap()
                        .unwrap_err()
                        .code,
                    "discovery-cancelled"
                );
                assert!(store.discovered_libraries(&data).unwrap().is_empty());
                let job = store.manager_job().unwrap().unwrap();
                assert_eq!(job["running"], false);
                assert_eq!(job["exit_code"], 1);
                assert!(job["log"].as_str().unwrap().contains("任务已取消"));
            } else {
                gate.0.lock().unwrap().0 = false;
                gate.1.notify_all();
                assert_eq!(
                    reply
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap()
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(store.discovered_libraries(&data).unwrap().len(), 1);
                let job = store.manager_job().unwrap().unwrap();
                assert_eq!(job["running"], false);
                assert_eq!(job["exit_code"], 0);
                assert!(job["log"].as_str().unwrap().contains("[TV] Z:\\TV"));
                assert!(job["log"].as_str().unwrap().contains(
                    "Emby 节点：TV  (Id=20, ParentId=2)\n  状态：离线/不可访问\n  证据：Series=1"
                ));
                assert!(store.configuration().unwrap().roots.is_empty());
                assert!(store.tasks().unwrap().is_empty());
                assert_eq!(worker.stop().unwrap().phase, "stopped");
                assert_eq!(
                    worker.request_discovery(data).unwrap_err().code,
                    "service-stopped"
                );
            }
        }
    }
}
