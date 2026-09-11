use crate::desktop::Desktop;
use product_core::{
    card_service::{CardService, ServiceStatus},
    emby::{self, Integration, IntegrationStatus, MaintenancePlan},
    *,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
const CARD_JS: &[u8] = include_bytes!("../../web-card/technical-specs-card.js");
const LANGUAGES: &[u8] = br#"{"schema":1,"catalog_app_version":"v4.1.0","languages":{}}"#;
#[derive(Default)]
struct Session {
    integration: Option<Arc<Integration>>,
    service: Option<CardService>,
}
pub struct EmbyDesktop {
    backup: PathBuf,
    session: Mutex<Session>,
}
impl EmbyDesktop {
    pub fn new(path: PathBuf) -> Self {
        Self {
            backup: path,
            session: Mutex::new(Session::default()),
        }
    }
    pub fn shutdown(&self) -> Result<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if let Some(service) = session.service.as_mut() {
            service.stop()?;
        }
        Ok(())
    }
}
fn public_catalog(store: &store::Store) -> Result<emby::PublicIndex> {
    // One authoritative SQLite snapshot prevents mixed revisions during a scan.
    Ok(emby::public_index(&store.all_items()?, emby::timestamp()))
}
#[tauri::command]
pub async fn emby_select(app: tauri::AppHandle) -> Result<Option<IntegrationStatus>> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("选择 Emby 的 dashboard-ui / web 目录")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|e| AppError::new("invalid-path", e))?;
        let state = app.state::<EmbyDesktop>();
        let mut session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.service.is_some() {
            return Err(AppError::new(
                "emby-stop-required",
                "Stop and disconnect the current instance first",
            ));
        }
        let integration = Arc::new(Integration::open(&path, &state.backup)?);
        let status = integration.status()?;
        session.integration = Some(integration);
        Ok(Some(status))
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub fn emby_status(state: State<'_, EmbyDesktop>) -> Result<Option<IntegrationStatus>> {
    let session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    session.integration.as_ref().map(|i| i.status()).transpose()
}
#[tauri::command]
pub fn emby_plan(
    id: String,
    action: String,
    state: State<'_, EmbyDesktop>,
    desktop: State<'_, Desktop>,
) -> Result<MaintenancePlan> {
    let session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    if session.service.is_some() {
        return Err(AppError::new(
            "emby-stop-required",
            "Stop the service before maintenance",
        ));
    }
    let integration = session
        .integration
        .as_ref()
        .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?;
    integration.plan(
        &id,
        &action,
        CARD_JS,
        &public_catalog(&desktop.store)?,
        LANGUAGES,
    )
}
#[tauri::command]
pub async fn emby_apply(
    id: String,
    fingerprint: String,
    app: tauri::AppHandle,
) -> Result<IntegrationStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.service.is_some() {
            return Err(AppError::new(
                "emby-stop-required",
                "Stop the service before maintenance",
            ));
        }
        session
            .integration
            .as_ref()
            .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?
            .apply(&id, &fingerprint)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub fn emby_start(id: String, state: State<'_, EmbyDesktop>) -> Result<ServiceStatus> {
    let mut session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    if let Some(service) = &session.service {
        return service.status();
    }
    let target = session
        .integration
        .clone()
        .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?;
    let service = CardService::start(target, &id)?;
    let status = service.status()?;
    session.service = Some(service);
    Ok(status)
}
#[tauri::command]
pub fn emby_stop(state: State<'_, EmbyDesktop>) -> Result<ServiceStatus> {
    let mut session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    if let Some(service) = session.service.as_mut() {
        let status = service.stop()?;
        session.service = None;
        return Ok(status);
    }
    Ok(ServiceStatus {
        phase: "stopped".into(),
        lease: None,
        error: None,
    })
}
#[tauri::command]
pub fn emby_service_status(state: State<'_, EmbyDesktop>) -> Result<ServiceStatus> {
    let session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    match session.service.as_ref() {
        Some(service) => service.status(),
        None => Ok(ServiceStatus {
            phase: "stopped".into(),
            lease: None,
            error: None,
        }),
    }
}
