//! Separate owners for lease renewal and index publication. Slow catalog I/O does
//! not hold the status lock or block renewal; stop joins publication then renewal.
use crate::{
    emby::{Integration, Lease},
    AppError, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ServiceStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<String>,
    pub phase: String,
    pub lease: Option<Lease>,
    pub error: Option<AppError>,
}
impl ServiceStatus {
    /// A starting service owns a live scanner, even while card display is withheld.
    pub fn active(&self) -> bool {
        matches!(self.phase.as_str(), "starting" | "running")
    }
}
struct Worker {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}
impl Worker {
    fn signal(&self) {
        let _ = self.stop.send(());
    }
    fn join(self) -> Result<()> {
        self.thread
            .join()
            .map_err(|_| AppError::new("emby-worker-panicked", "Card worker panicked"))
    }
}
pub struct CardService {
    integration: Arc<Integration>,
    state: Arc<Mutex<ServiceStatus>>,
    publisher: Option<Worker>,
    renewal: Option<Worker>,
    publication_gate: Arc<Mutex<()>>,
    lease_gate: Arc<Mutex<()>>,
    index_permitted: Arc<std::sync::atomic::AtomicBool>,
}
fn snapshot(state: &Mutex<ServiceStatus>) -> Result<ServiceStatus> {
    state
        .lock()
        .map(|s| s.clone())
        .map_err(|e| AppError::new("emby-state", e))
}
fn failed(state: &Mutex<ServiceStatus>, error: AppError) {
    if let Ok(mut state) = state.lock() {
        state.phase = "failed".into();
        state.error = Some(error);
    }
}
fn tick(receiver: &mpsc::Receiver<()>) -> bool {
    matches!(
        receiver.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Timeout)
    )
}
impl CardService {
    pub fn start(integration: Arc<Integration>, session: &str) -> Result<Self> {
        Self::start_inner(integration, session, None, true)
    }
    pub fn start_waiting_for_index(integration: Arc<Integration>, session: &str) -> Result<Self> {
        Self::start_inner(integration, session, None, false)
    }
    pub fn start_with_store(
        integration: Arc<Integration>,
        session: &str,
        store: Arc<crate::store::Store>,
    ) -> Result<Self> {
        Self::start_inner(integration, session, Some(store), false)
    }
    fn start_inner(
        integration: Arc<Integration>,
        session: &str,
        source: Option<Arc<crate::store::Store>>,
        permit: bool,
    ) -> Result<Self> {
        let observed = integration.status()?;
        let fingerprint = observed
            .details
            .and_then(|details| details.data_fingerprint);
        let enabled = match &source {
            Some(store) => store.index_current(fingerprint.as_deref())?,
            None => permit,
        };
        let lease = integration.begin_session_enabled(session, enabled)?;
        let published = Arc::new(Mutex::new(fingerprint));
        let index_permitted = Arc::new(std::sync::atomic::AtomicBool::new(enabled));
        let lease_gate = Arc::new(Mutex::new(()));
        let state = Arc::new(Mutex::new(ServiceStatus {
            phase: if enabled { "running" } else { "starting" }.into(),
            last_started_at: enabled.then(|| lease.updated_at.clone()),
            lease: Some(lease),
            error: None,
        }));
        // Own the first worker before starting the second, so any startup failure
        // drops this owner, joins existing workers and disables the lease.
        let publication_gate = Arc::new(Mutex::new(()));
        let mut service = Self {
            integration: integration.clone(),
            state: state.clone(),
            publisher: None,
            renewal: None,
            publication_gate: publication_gate.clone(),
            lease_gate: lease_gate.clone(),
            index_permitted: index_permitted.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        let target = integration.clone();
        let lease_source = source.clone();
        let lease_index = published.clone();
        let thread = std::thread::Builder::new()
            .name("emby-lease".into())
            .spawn(move || {
                while tick(&rx) {
                    let _gate = match lease_gate.lock() {
                        Ok(gate) => gate,
                        Err(_) => return,
                    };
                    let current = match snapshot(&shared) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let Some(ref lease) = current.lease else {
                        return;
                    };
                    let allowed = match &lease_source {
                        Some(store) => lease_index
                            .lock()
                            .map_err(|e| AppError::new("emby-publication-state", e))
                            .and_then(|index| store.index_current(index.as_deref())),
                        None => Ok(index_permitted.load(std::sync::atomic::Ordering::SeqCst)),
                    };
                    let active = current.active();
                    let enabled = match allowed {
                        Ok(allowed) => active && allowed,
                        Err(error) => {
                            failed(&shared, error);
                            false
                        }
                    };
                    let result = target.renew_session(
                        &lease.session_id,
                        lease.sequence.saturating_add(1),
                        enabled,
                    );
                    match result {
                        Ok(lease) => {
                            if let Ok(mut state) = shared.lock() {
                                if lease.enabled && state.phase == "starting" {
                                    state.phase = "running".into();
                                    state.last_started_at = Some(lease.updated_at.clone());
                                }
                                state.lease = Some(lease);
                            }
                        }
                        Err(error) => {
                            failed(&shared, error);
                            return;
                        }
                    }
                    if !active {
                        return;
                    }
                }
            })
            .map_err(|e| AppError::new("emby-worker-start", e))?;
        service.renewal = Some(Worker { stop: tx, thread });
        let (tx, rx) = mpsc::channel();
        let shared = state;
        let thread = std::thread::Builder::new()
            .name("emby-publish".into())
            .spawn(move || {
                let mut revision = None;
                while tick(&rx) {
                    if !snapshot(&shared).is_ok_and(|state| state.active()) {
                        return;
                    }
                    let result = (|| -> Result<()> {
                        let _publication = publication_gate
                            .lock()
                            .map_err(|error| AppError::new("emby-publication-state", error))?;
                        let observed = integration.require_index_access()?;
                        if observed.details.is_some_and(|details| !details.data_exists) {
                            *published
                                .lock()
                                .map_err(|e| AppError::new("emby-publication-state", e))? = None;
                            revision = None;
                        }
                        if let Some(store) = &source {
                            if let Some((next, items)) = store.publication_snapshot(revision)? {
                                let index =
                                    crate::emby::public_index(&items, crate::emby::timestamp());
                                integration.publish_index(&index)?;
                                *published
                                    .lock()
                                    .map_err(|e| AppError::new("emby-publication-state", e))? =
                                    Some(crate::emby::index_fingerprint(&index)?);
                                revision = Some(next);
                            }
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        failed(&shared, error);
                        return;
                    }
                }
            })
            .map_err(|e| AppError::new("emby-worker-start", e))?;
        service.publisher = Some(Worker { stop: tx, thread });
        Ok(service)
    }
    /// Keep renewal and the original session alive while files are maintained.
    pub fn repair_web(
        &self,
        id: &str,
        javascript: &[u8],
        languages: &[u8],
    ) -> Result<crate::emby::IntegrationStatus> {
        let _publication = self
            .publication_gate
            .lock()
            .map_err(|error| AppError::new("emby-publication-state", error))?;
        self.integration.repair_web(id, javascript, languages)
    }
    pub fn publish_index(&self, index: &crate::emby::PublicIndex) -> Result<bool> {
        // A helper publication shares the same owner as repair and background
        // observation. Its prepared journal must not be mistaken for a crash.
        let _publication = self
            .publication_gate
            .lock()
            .map_err(|error| AppError::new("emby-publication-state", error))?;
        self.integration.publish_index(index)
    }
    pub fn status(&self) -> Result<ServiceStatus> {
        snapshot(&self.state)
    }
    /// The authenticated Manager supplies a completed-index fingerprint, or None
    /// to withhold display. The helper verifies the actual installed data itself.
    pub fn set_index_current(&self, expected: Option<&str>) -> Result<ServiceStatus> {
        let _gate = self
            .lease_gate
            .lock()
            .map_err(|e| AppError::new("emby-lease-state", e))?;
        let current = snapshot(&self.state)?;
        if !current.active() {
            return Ok(current);
        }
        self.integration.require_index_access()?;
        let observed = self.integration.status()?;
        let actual = observed
            .details
            .and_then(|details| details.data_fingerprint);
        let enabled = expected
            .zip(actual.as_deref())
            .is_some_and(|(expected, actual)| expected == actual);
        self.index_permitted
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
        if let Some(lease) = current.lease {
            if lease.enabled != enabled {
                let next = self.integration.renew_session(
                    &lease.session_id,
                    lease.sequence.saturating_add(1),
                    enabled,
                )?;
                let mut state = self
                    .state
                    .lock()
                    .map_err(|e| AppError::new("emby-state", e))?;
                if next.enabled && state.phase == "starting" {
                    state.phase = "running".into();
                    state.last_started_at = Some(next.updated_at.clone());
                }
                state.lease = Some(next);
            }
        }
        snapshot(&self.state)
    }
    pub fn stop(&mut self) -> Result<ServiceStatus> {
        self.stop_with_timeout(Duration::from_secs(8))
    }
    fn stop_with_timeout(&mut self, timeout: Duration) -> Result<ServiceStatus> {
        // Signal both owners before waiting. Do not withdraw the lease until both
        // are joined: a publisher may still be inside file I/O and a renewal may
        // otherwise race a premature disabled write.
        for worker in [self.publisher.as_ref(), self.renewal.as_ref()]
            .into_iter()
            .flatten()
        {
            worker.signal();
        }
        let deadline = Instant::now() + timeout;
        let publisher_error = match wait_for_worker(&mut self.publisher, deadline, "发布") {
            Ok(error) => error,
            Err(error) => {
                failed(&self.state, error.clone());
                return Err(error);
            }
        };
        let renewal_error = match wait_for_worker(&mut self.renewal, deadline, "续租") {
            Ok(error) => error,
            Err(error) => {
                failed(&self.state, error.clone());
                return Err(error);
            }
        };
        let current = snapshot(&self.state)?;
        if current.phase == "stopped" {
            return Ok(current);
        }
        if let Some(lease) = current.lease {
            match self.integration.renew_session(
                &lease.session_id,
                lease.sequence.saturating_add(1),
                false,
            ) {
                Ok(lease) => {
                    self.state
                        .lock()
                        .map_err(|e| AppError::new("emby-state", e))?
                        .lease = Some(lease)
                }
                Err(error) => {
                    failed(&self.state, error.clone());
                    return Err(error);
                }
            }
        }
        if let Some(error) = publisher_error.or(renewal_error) {
            failed(&self.state, error.clone());
            return Err(error);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        state.phase = "stopped".into();
        state.error = None;
        Ok(state.clone())
    }
}
impl Drop for CardService {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Emby stop failed; last lease must expire: {error}");
            if error.code == "emby-worker-stop-timeout" {
                // Explicit stop retains timed-out owners for a visible retry. If
                // this service owner is itself destroyed, synchronously reclaim
                // them instead of detaching workers that can publish or renew.
                for worker in [self.publisher.take(), self.renewal.take()]
                    .into_iter()
                    .flatten()
                {
                    worker.signal();
                    if let Err(error) = worker.join() {
                        eprintln!("Emby stop failed; last lease must expire: {error}");
                    }
                }
                if let Err(error) = self.stop() {
                    eprintln!("Emby stop failed; last lease must expire: {error}");
                }
            }
        }
    }
}

fn wait_for_worker(
    worker: &mut Option<Worker>,
    deadline: Instant,
    label: &str,
) -> Result<Option<AppError>> {
    let Some(active) = worker.as_ref() else {
        return Ok(None);
    };
    while !active.thread.is_finished() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AppError::new(
                "emby-worker-stop-timeout",
                format!("卡片服务{label}线程停止等待超过 8 秒，请稍后重试退出"),
            ));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    Ok(worker.take().and_then(|worker| worker.join().err()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maintenance_waits_for_publication_while_lease_renewal_remains_live() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let web = directory.join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("index.html"),
            format!(
                "<html><head></head><body>{}</body></html>",
                "Emby ".repeat(80)
            ),
        )
        .unwrap();
        let integration = Arc::new(Integration::open(&web, &directory.join("private")).unwrap());
        let languages = crate::emby::bundled_card_languages().unwrap();
        integration
            .repair_web("setup", b"old script", &languages)
            .unwrap();
        let service = Arc::new(CardService::start(integration, "uninterrupted").unwrap());
        let before = service.status().unwrap();
        let held = service.publication_gate.lock().unwrap();
        let queued = service.clone();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = finished.clone();
        let repair = std::thread::spawn(move || {
            let result = queued.repair_web("queued-repair", b"new script", &languages);
            signal.store(true, std::sync::atomic::Ordering::SeqCst);
            result
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let during = loop {
            let observed = service.status().unwrap();
            assert_eq!(observed.phase, "running", "{observed:?}");
            if observed.lease.as_ref().unwrap().sequence > before.lease.as_ref().unwrap().sequence {
                break observed;
            }
            assert!(Instant::now() < deadline, "lease renewal timed out");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(during.phase, "running");
        assert_eq!(during.last_started_at, before.last_started_at);
        assert!(during.lease.unwrap().sequence > before.lease.unwrap().sequence);
        assert!(!finished.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            std::fs::read(web.join("technical-specs-card.js")).unwrap(),
            b"old script"
        );
        drop(held);
        assert!(repair.join().unwrap().unwrap().healthy);
        assert_eq!(
            std::fs::read(web.join("technical-specs-card.js")).unwrap(),
            b"new script"
        );
        let mut service = match Arc::try_unwrap(service) {
            Ok(value) => value,
            Err(_) => panic!("repair owner was not released"),
        };
        service.stop().unwrap();
    }

    #[test]
    fn timed_out_stop_keeps_the_blocked_publisher_for_retry() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let web = directory.join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("index.html"),
            format!(
                "<html><head></head><body>{}</body></html>",
                "Emby ".repeat(80)
            ),
        )
        .unwrap();
        let integration = Arc::new(Integration::open(&web, &directory.join("private")).unwrap());
        let languages = crate::emby::bundled_card_languages().unwrap();
        integration
            .repair_web("setup", b"card script", &languages)
            .unwrap();
        let mut service = CardService::start(integration, "stop-retry").unwrap();
        let publication_gate = service.publication_gate.clone();
        let held = publication_gate.lock().unwrap();
        std::thread::sleep(Duration::from_millis(2200));

        let error = service
            .stop_with_timeout(Duration::from_millis(20))
            .unwrap_err();
        assert_eq!(error.code, "emby-worker-stop-timeout");
        assert!(service.publisher.is_some());
        assert_eq!(service.status().unwrap().phase, "failed");
        assert_eq!(
            service.status().unwrap().error.unwrap().code,
            "emby-worker-stop-timeout"
        );

        drop(held);
        assert_eq!(
            service
                .stop_with_timeout(Duration::from_secs(2))
                .unwrap()
                .phase,
            "stopped"
        );
        assert!(service.publisher.is_none());
        assert!(service.renewal.is_none());
    }
}
