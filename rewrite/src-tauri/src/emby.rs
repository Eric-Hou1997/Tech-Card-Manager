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
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
const CARD_JS: &[u8] = include_bytes!("../../web-card/technical-specs-card.js");
#[derive(Default)]
struct Session {
    integration: Option<Arc<Integration>>,
    service: Option<CardService>,
    restore_error: Option<AppError>,
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
    pub fn restore(&self, store: &store::Store) -> Result<()> {
        let value = store.preferences("emby-environment")?;
        if value.get("web").is_none() {
            return Ok(());
        }
        let environment: product_core::emby_environment::Environment =
            serde_json::from_value(value)?;
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        match Integration::open(std::path::Path::new(&environment.web), &self.backup) {
            Ok(integration) => session.integration = Some(Arc::new(integration)),
            Err(error) => session.restore_error = Some(error),
        }
        Ok(())
    }
    pub fn shutdown(&self) -> Result<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if let Some(service) = session.service.as_mut() {
            service.stop()?;
        }
        session.service = None;
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
        let environment = product_core::emby_environment::inspect(&path)?;
        connect_environment(environment, &app).map(Some)
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
        &emby::bundled_card_languages()?,
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
pub fn emby_start(
    id: String,
    state: State<'_, EmbyDesktop>,
    desktop: State<'_, Desktop>,
) -> Result<ServiceStatus> {
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
    let service = CardService::start_with_store(target, &id, desktop.store.clone())?;
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
#[tauri::command]
pub fn emby_discover(
    app: tauri::AppHandle,
) -> Result<Vec<product_core::emby_environment::Environment>> {
    let home = app
        .path()
        .home_dir()
        .map_err(|e| AppError::new("home-directory", e))?;
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    Ok(product_core::emby_environment::discover(
        std::env::consts::OS,
        &home,
        roaming.as_deref(),
        local.as_deref(),
    ))
}
#[tauri::command]
pub fn emby_environment(
    desktop: State<'_, Desktop>,
    state: State<'_, EmbyDesktop>,
) -> Result<serde_json::Value> {
    let mut value = desktop.store.preferences("emby-environment")?;
    let session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    if let Some(error) = &session.restore_error {
        value["restore_error"] = serde_json::to_value(error)?;
    }
    Ok(value)
}
#[tauri::command]
pub fn emby_connect(path: String, app: tauri::AppHandle) -> Result<IntegrationStatus> {
    let environment = product_core::emby_environment::inspect(std::path::Path::new(&path))?;
    connect_environment(environment, &app)
}
fn connect_environment(
    environment: product_core::emby_environment::Environment,
    app: &tauri::AppHandle,
) -> Result<IntegrationStatus> {
    let state = app.state::<EmbyDesktop>();
    let mut session = state
        .session
        .lock()
        .map_err(|e| AppError::new("emby-state", e))?;
    if session.service.is_some() {
        return Err(AppError::new(
            "emby-stop-required",
            "Stop the current card service before changing its installation",
        ));
    }
    if let Some(integration) = session.integration.as_ref() {
        if integration.status()?.target == environment.web {
            return integration.status();
        }
    }
    let integration = Arc::new(Integration::open(
        std::path::Path::new(&environment.web),
        &state.backup,
    )?);
    let status = integration.status()?;
    app.state::<Desktop>()
        .store
        .save_preference("emby-environment", &serde_json::to_value(&environment)?)?;
    session.integration = Some(integration);
    session.restore_error = None;
    Ok(status)
}
#[tauri::command]
pub async fn emby_check_server(
    endpoint: String,
    app: tauri::AppHandle,
) -> Result<product_core::emby_environment::Environment> {
    let url = product_core::emby_environment::endpoint(&endpoint)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::new("emby-network", e))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::new("emby-network", e))?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "emby-server-http",
            format!("Emby returned HTTP {}", response.status()),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::new("emby-network", e))?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(AppError::new(
                "emby-server-response",
                "Response exceeds 1 MiB",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let store = &app.state::<Desktop>().store;
    let mut environment: product_core::emby_environment::Environment =
        serde_json::from_value(store.preferences("emby-environment")?)?;
    product_core::emby_environment::server_version(
        &mut environment,
        &serde_json::from_slice(&bytes)?,
    )?;
    environment.endpoint = endpoint;
    store.save_preference("emby-environment", &serde_json::to_value(&environment)?)?;
    Ok(environment)
}
#[tauri::command]
pub async fn emby_data_directory(
    app: tauri::AppHandle,
) -> Result<Option<product_core::emby_environment::Environment>> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("选择 Emby 数据目录 / Select Emby data directory")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|e| AppError::new("invalid-path", e))?;
        let real = product_core::paths::checked(&path)?;
        if !real.join("config").is_dir() && !real.join("data").is_dir() {
            return Err(AppError::new(
                "emby-data-directory",
                "Selected folder contains neither config nor data",
            )
            .at(real.display()));
        }
        let store = &app.state::<Desktop>().store;
        let mut environment: product_core::emby_environment::Environment =
            serde_json::from_value(store.preferences("emby-environment")?)?;
        environment.data = Some(real.to_string_lossy().into());
        store.save_preference("emby-environment", &serde_json::to_value(&environment)?)?;
        Ok(Some(environment))
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}

#[tauri::command]
pub fn emby_path_mappings(
    state: State<'_, Desktop>,
) -> Result<product_core::emby_libraries::MappingSettings> {
    state.store.path_mappings()
}
#[tauri::command]
pub async fn emby_save_mappings(
    id: String,
    value: product_core::emby_libraries::MappingSettings,
    app: tauri::AppHandle,
) -> Result<product_core::emby_libraries::MappingSettings> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Desktop>().store.set_path_mappings(&id, value)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub async fn emby_libraries(
    app: tauri::AppHandle,
) -> Result<Vec<product_core::emby_libraries::DiscoveredLibrary>> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = &app.state::<Desktop>().store;
        let environment: product_core::emby_environment::Environment =
            serde_json::from_value(store.preferences("emby-environment")?)?;
        let data = environment.data.ok_or_else(|| {
            AppError::new("emby-data-directory", "Select Emby data directory first")
        })?;
        product_core::emby_libraries::discover(
            std::path::Path::new(&data),
            environment.version.as_deref().unwrap_or(""),
            &store.path_mappings()?.mappings,
        )
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub async fn emby_add_library(
    id: String,
    path: String,
    space: Space,
    app: tauri::AppHandle,
) -> Result<Configuration> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = &app.state::<Desktop>().store;
        match store.operation_result(&id) {
            Ok(OperationResult::Configuration(value)) => return Ok(value),
            Ok(_) => {
                return Err(AppError::new(
                    "operation-conflict",
                    "ID belongs to another action",
                ))
            }
            Err(e) if e.code == "operation-not-found" => {}
            Err(e) => return Err(e),
        }
        let real = product_core::paths::checked(std::path::Path::new(&path))?;
        let path = real
            .to_str()
            .ok_or_else(|| AppError::new("path-encoding", "Path must be Unicode"))?
            .to_owned();
        let mut configuration = store.configuration()?;
        configuration.roots.push(LibraryRoot {
            id: product_core::hash(
                format!(
                    "{}:{path}",
                    if space == Space::Movie { "movie" } else { "tv" }
                )
                .as_bytes(),
            ),
            path,
            space,
        });
        let value = store.configure(&id, configuration)?;
        if let Err(error) = app.emit("configuration-changed", &value) {
            eprintln!("configuration-event: {error}");
        }
        Ok(value)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
