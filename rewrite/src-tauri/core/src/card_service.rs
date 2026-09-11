//! One owner for the renewal worker. Stop joins it before publishing disabled.
use crate::{
    emby::{Integration, Lease},
    AppError, Result,
};
use serde::Serialize;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub phase: String,
    pub lease: Option<Lease>,
    pub error: Option<AppError>,
}
pub struct CardService {
    integration: Arc<Integration>,
    state: Arc<Mutex<ServiceStatus>>,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
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
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        let target = integration.clone();
        let worker = std::thread::Builder::new()
            .name("emby-lease".into())
            .spawn(move || {
                while matches!(
                    rx.recv_timeout(Duration::from_secs(2)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let mut state = match shared.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let Some(current) = state.lease.as_ref() else {
                        return;
                    };
                    let result = target.status().and_then(|health| {
                        if !health.healthy {
                            return Err(AppError::new(
                                "emby-repair-required",
                                health.issues.join(", "),
                            ));
                        }
                        if let Some(store) = &source {
                            if store.tasks()?.iter().all(|task| task.state.terminal()) {
                                let index = crate::emby::public_index(
                                    &store.all_items()?,
                                    crate::emby::timestamp(),
                                );
                                target.publish_index(&index)?;
                            }
                        }
                        target.renew_session(&current.session_id, current.sequence + 1, true)
                    });
                    match result {
                        Ok(next) => state.lease = Some(next),
                        Err(error) => {
                            state.phase = "failed".into();
                            state.error = Some(error);
                            return;
                        }
                    }
                }
            })
            .map_err(|e| {
                if let Ok(state) = state.lock() {
                    if let Some(lease) = &state.lease {
                        let _ =
                            integration.renew_session(&lease.session_id, lease.sequence + 1, false);
                    }
                }
                AppError::new("emby-worker-start", e)
            })?;
        Ok(Self {
            integration,
            state,
            stop: Some(tx),
            worker: Some(worker),
        })
    }
    pub fn status(&self) -> Result<ServiceStatus> {
        self.state
            .lock()
            .map(|s| s.clone())
            .map_err(|e| AppError::new("emby-state", e))
    }
    pub fn stop(&mut self) -> Result<ServiceStatus> {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| AppError::new("emby-worker-panicked", "Lease worker panicked"))?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if state.phase == "stopped" {
            return Ok(state.clone());
        }
        if let Some(current) = state.lease.as_ref() {
            match self
                .integration
                .renew_session(&current.session_id, current.sequence + 1, false)
            {
                Ok(lease) => state.lease = Some(lease),
                Err(error) => {
                    state.phase = "failed".into();
                    state.error = Some(error.clone());
                    return Err(error);
                }
            }
        }
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
