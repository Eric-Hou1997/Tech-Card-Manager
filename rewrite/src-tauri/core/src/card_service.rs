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
    time::Duration,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub phase: String,
    pub lease: Option<Lease>,
    pub error: Option<AppError>,
}
struct Worker {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}
impl Worker {
    fn join(self) -> Result<()> {
        let _ = self.stop.send(());
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
        Self::start_inner(integration, session, None)
    }
    pub fn start_with_store(
        integration: Arc<Integration>,
        session: &str,
        store: Arc<crate::store::Store>,
    ) -> Result<Self> {
        Self::start_inner(integration, session, Some(store))
    }
    fn start_inner(
        integration: Arc<Integration>,
        session: &str,
        source: Option<Arc<crate::store::Store>>,
    ) -> Result<Self> {
        let lease = integration.begin_session(session)?;
        let state = Arc::new(Mutex::new(ServiceStatus {
            phase: "running".into(),
            lease: Some(lease),
            error: None,
        }));
        // Own the first worker before starting the second, so any startup failure
        // drops this owner, joins existing workers and disables the lease.
        let mut service = Self {
            integration: integration.clone(),
            state: state.clone(),
            publisher: None,
            renewal: None,
        };
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        let target = integration.clone();
        let thread = std::thread::Builder::new()
            .name("emby-lease".into())
            .spawn(move || {
                while tick(&rx) {
                    let current = match snapshot(&shared) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let Some(lease) = current.lease else { return };
                    let enabled = current.phase == "running";
                    let result = target.renew_session(
                        &lease.session_id,
                        lease.sequence.saturating_add(1),
                        enabled,
                    );
                    match result {
                        Ok(lease) => {
                            if let Ok(mut state) = shared.lock() {
                                state.lease = Some(lease);
                            }
                        }
                        Err(error) => {
                            failed(&shared, error);
                            return;
                        }
                    }
                    if !enabled {
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
                    if !snapshot(&shared).is_ok_and(|state| state.phase == "running") {
                        return;
                    }
                    let result = (|| -> Result<()> {
                        let health = integration.status()?;
                        if !health.healthy {
                            return Err(AppError::new(
                                "emby-repair-required",
                                health.issues.join(", "),
                            ));
                        }
                        if let Some(store) = &source {
                            if let Some((next, items)) = store.publication_snapshot(revision)? {
                                let index =
                                    crate::emby::public_index(&items, crate::emby::timestamp());
                                integration.publish_index(&index)?;
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
    pub fn status(&self) -> Result<ServiceStatus> {
        snapshot(&self.state)
    }
    pub fn stop(&mut self) -> Result<ServiceStatus> {
        // Keep renewing while an already-started publication finishes. Always join
        // both workers even if one panicked, then attempt to disable the runtime.
        let publisher_error = self.publisher.take().and_then(|w| w.join().err());
        let renewal_error = self.renewal.take().and_then(|w| w.join().err());
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
        }
    }
}
