use crate::desktop::Desktop;
use product_core::{
    card_service::{CardService, ServiceStatus},
    emby::{self, Integration, IntegrationStatus, MaintenancePlan},
    *,
};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
const CARD_JS: &[u8] = include_bytes!("../../web-card/technical-specs-card.js");
#[derive(Default)]
struct Session {
    closing: bool,
    scan_session: Option<String>,
    initial_failure: Option<ServiceStatus>,
    integration: Option<Arc<Integration>>,
    service: Option<CardService>,
    restore_error: Option<AppError>,
    privileged: Option<crate::privileged::Connection>,
}
impl Session {
    fn service_status(&self) -> Result<ServiceStatus> {
        if let Some(failure) = &self.initial_failure {
            return Ok(failure.clone());
        }
        if let Some(remote) = &self.privileged {
            return match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { service, .. } => Ok(service),
                _ => Err(helper_reply()),
            };
        }
        self.service
            .as_ref()
            .map(CardService::status)
            .unwrap_or_else(|| {
                Ok(ServiceStatus {
                    phase: "stopped".into(),
                    last_started_at: None,
                    lease: None,
                    error: None,
                })
            })
    }
    fn stop_service(&mut self) -> Result<ServiceStatus> {
        let status = if let Some(remote) = &self.privileged {
            stop_remote_service(|command| remote.request(command))?
        } else if let Some(service) = self.service.as_mut() {
            service.stop()?
        } else {
            ServiceStatus {
                phase: "stopped".into(),
                last_started_at: None,
                lease: None,
                error: None,
            }
        };
        if status.phase != "stopped"
            || status.error.is_some()
            || status.lease.as_ref().is_some_and(|lease| lease.enabled)
        {
            return Err(status.error.unwrap_or_else(|| {
                AppError::new(
                    "service-stop-unverified",
                    "服务停止尚未确认，请重试停止服务",
                )
            }));
        }
        self.service = None;
        self.initial_failure = None;
        Ok(status)
    }
    fn fail_initial_index(&mut self, identity: &str, error: AppError) -> Result<()> {
        if self.closing || self.scan_session.as_deref() != Some(identity) {
            return Ok(());
        }
        let status = self.service_status()?;
        if status.phase == "starting" {
            // Stop only the publication/lease owner here, never the calling scanner.
            self.initial_failure =
                Some(initial_index_failure(status, error, || self.stop_service()));
        }
        Ok(())
    }
    fn require_running(&self) -> Result<()> {
        if self.closing {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        let running = if let Some(remote) = &self.privileged {
            match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { service, .. } => service.active(),
                _ => return Err(helper_reply()),
            }
        } else {
            self.service
                .as_ref()
                .map(|service| service.status().map(|s| s.active()))
                .transpose()?
                .unwrap_or(false)
        };
        if !running {
            return Err(AppError::new("service-stopped", "服务已停止，请先启动服务"));
        }
        Ok(())
    }
}
pub(crate) fn stop_remote_service(
    request: impl FnOnce(
        product_core::maintenance::Command,
    ) -> Result<product_core::maintenance::Outcome>,
) -> Result<ServiceStatus> {
    match request(product_core::maintenance::Command::Stop {})? {
        product_core::maintenance::Outcome::Service(status) => Ok(status),
        _ => Err(helper_reply()),
    }
}
pub(crate) fn initial_index_failure(
    mut status: ServiceStatus,
    mut error: AppError,
    stop: impl FnOnce() -> Result<ServiceStatus>,
) -> ServiceStatus {
    match stop() {
        Ok(stopped)
            if stopped.phase == "stopped"
                && stopped.error.is_none()
                && !stopped.lease.as_ref().is_some_and(|lease| lease.enabled) =>
        {
            status = stopped;
            status.phase = "error".into();
        }
        Ok(unverified) => {
            status = unverified;
            status.phase = "failed".into();
            error.message = format!(
                "{}；停止服务尚未确认：{}",
                error.message,
                status
                    .error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "service-stop-unverified".into())
            );
        }
        Err(cleanup) => {
            status.phase = "failed".into();
            error.message = format!("{}；停止服务失败：{}", error.message, cleanup);
        }
    }
    status.error = Some(error);
    status
}
pub struct EmbyDesktop {
    scanner: Mutex<Option<product_core::incremental::IncrementalWorker>>,
    backup: PathBuf,
    session: Mutex<Session>,
    maintenance: Arc<MaintenanceGate>,
}

#[derive(Default)]
struct MaintenanceGate {
    active: Mutex<Option<MaintenanceContext>>,
    changed: Condvar,
}

#[derive(Clone)]
pub(crate) struct MaintenanceContext {
    pub phase: String,
    pub legacy_blocked: bool,
}

struct MaintenanceWork(Arc<MaintenanceGate>);

impl Drop for MaintenanceWork {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            *active = None;
            self.0.changed.notify_all();
        }
    }
}

impl EmbyDesktop {
    pub fn new(path: PathBuf) -> Self {
        Self {
            backup: path,
            scanner: Mutex::new(None),
            session: Mutex::new(Session::default()),
            maintenance: Arc::new(MaintenanceGate::default()),
        }
    }
    fn begin_maintenance(
        &self,
        service: ServiceStatus,
        legacy_blocked: bool,
    ) -> Result<MaintenanceWork> {
        let mut active = self
            .maintenance
            .active
            .lock()
            .map_err(|e| AppError::new("maintenance-state", e))?;
        if active.is_some() {
            return Err(AppError::new("task-busy", "已有管理员维护事务正在执行"));
        }
        *active = Some(MaintenanceContext {
            phase: service.phase,
            legacy_blocked,
        });
        drop(active);
        Ok(MaintenanceWork(self.maintenance.clone()))
    }
    pub(crate) fn maintenance_context(&self) -> Result<Option<MaintenanceContext>> {
        self.maintenance
            .active
            .lock()
            .map(|active| active.clone())
            .map_err(|e| AppError::new("maintenance-state", e))
    }
    fn wait_for_maintenance(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut active = self
            .maintenance
            .active
            .lock()
            .map_err(|e| AppError::new("maintenance-state", e))?;
        while active.is_some() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AppError::new(
                    "maintenance-exit-timeout",
                    "管理员维护尚未结束，退出已暂停；事务仍由当前程序持有，没有遗留到后台",
                ));
            }
            let (next, waited) = self
                .maintenance
                .changed
                .wait_timeout(active, remaining)
                .map_err(|e| AppError::new("maintenance-state", e))?;
            active = next;
            if waited.timed_out() && active.is_some() {
                return Err(AppError::new(
                    "maintenance-exit-timeout",
                    "管理员维护尚未结束，退出已暂停；事务仍由当前程序持有，没有遗留到后台",
                ));
            }
        }
        Ok(())
    }
    fn record_startup_failure(&self, error: AppError) -> Result<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.closing || session.service_status()?.active() {
            return Ok(());
        }
        session.initial_failure = Some(ServiceStatus {
            phase: "error".into(),
            last_started_at: None,
            lease: None,
            error: Some(error),
        });
        Ok(())
    }
    pub(crate) fn service_status(&self) -> Result<ServiceStatus> {
        self.session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?
            .service_status()
    }
    fn stop_incremental(&self) -> Result<()> {
        self.session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?
            .scan_session = None;
        if let Some(scanner) = self
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?
            .as_mut()
        {
            scanner.stop()?;
        }
        Ok(())
    }
    fn stop_for_user(&self) -> Result<ServiceStatus> {
        let mut failure = self.stop_incremental().err();
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        let mut status = match session.stop_service() {
            Ok(status) => status,
            Err(error) => {
                append_shutdown_error(&mut failure, error);
                match session.service_status() {
                    Ok(status) => status,
                    Err(error) => {
                        append_shutdown_error(&mut failure, error);
                        ServiceStatus {
                            phase: "failed".into(),
                            last_started_at: None,
                            lease: None,
                            error: None,
                        }
                    }
                }
            }
        };
        if let Some(error) = failure {
            status.phase = "failed".into();
            status.error = Some(error);
            // Keep one stable stop-failure observation until the same visible
            // action retries scanner cleanup and card removal successfully.
            session.initial_failure = Some(status.clone());
        }
        Ok(status)
    }
    fn start_incremental(&self, app: &tauri::AppHandle, id: &str) -> Result<()> {
        let mut scanner = self
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        if let Some(worker) = scanner.as_mut() {
            if matches!(worker.status()?.phase.as_str(), "waiting" | "checking") {
                return Ok(());
            }
            worker.stop()?;
        }
        let check = app.clone();
        let events = app.clone();
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let identity = product_core::hash(&serde_json::to_vec(&(
            id,
            emby::timestamp(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ))?);
        {
            let mut session = self
                .session
                .lock()
                .map_err(|e| AppError::new("emby-state", e))?;
            session.require_running()?;
            session.scan_session = Some(identity.clone());
        }
        let completion = app.clone();
        let completed_identity = identity.clone();
        let observed = Mutex::new(None::<(std::time::Instant, bool)>);
        let started = product_core::incremental::IncrementalWorker::start_with_completion(
            app.state::<Desktop>().store.clone(),
            &identity,
            move || {
                let mut observed = observed
                    .lock()
                    .map_err(|e| AppError::new("incremental-state", e))?;
                if let Some((at, running)) = *observed {
                    if at.elapsed() < std::time::Duration::from_secs(2) {
                        return Ok(running);
                    }
                }
                let owner = check.state::<EmbyDesktop>();
                let session = match owner.session.try_lock() {
                    Ok(value) => value,
                    // Explicit stop sets the scanner's cancellation flag before
                    // acquiring this lock. A concurrent short status query does not
                    // revoke an otherwise confirmed live service.
                    Err(std::sync::TryLockError::WouldBlock) => return Ok(true),
                    Err(error) => return Err(AppError::new("emby-state", error)),
                };
                if let Some(remote) = &session.privileged {
                    return match remote.request(product_core::maintenance::Command::Status {})? {
                        product_core::maintenance::Outcome::Status { service, .. } => {
                            let running = service.active();
                            *observed = Some((std::time::Instant::now(), running));
                            Ok(running)
                        }
                        _ => Err(helper_reply()),
                    };
                }
                let running = session
                    .service
                    .as_ref()
                    .map(|service| service.status().map(|status| status.active()))
                    .unwrap_or(Ok(false))?;
                *observed = Some((std::time::Instant::now(), running));
                Ok(running)
            },
            move |task| {
                let _ = events.emit("task-changed", task);
            },
            move |terminal| {
                if let Some(error) = terminal.error {
                    let owner = completion.state::<EmbyDesktop>();
                    let result = owner
                        .session
                        .lock()
                        .map_err(|e| AppError::new("emby-state", e))
                        .and_then(|mut session| {
                            session.fail_initial_index(&completed_identity, error)
                        });
                    if let Err(error) = result {
                        eprintln!("initial card index: {error}");
                    }
                }
            },
        );
        match started {
            Ok(worker) => *scanner = Some(worker),
            Err(error) => {
                self.session
                    .lock()
                    .map_err(|e| AppError::new("emby-state", e))?
                    .fail_initial_index(&identity, error.clone())?;
                return Err(error);
            }
        }
        Ok(())
    }
    #[cfg(test)]
    fn restore(
        &self,
        store: &store::Store,
        discover: impl FnOnce() -> Result<Vec<product_core::emby_environment::Environment>>,
    ) -> Result<()> {
        self.restore_with_import(store, discover, |_| Ok(()))
    }
    fn restore_with_import(
        &self,
        store: &store::Store,
        discover: impl FnOnce() -> Result<Vec<product_core::emby_environment::Environment>>,
        import: impl FnOnce(&str) -> Result<()>,
    ) -> Result<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.closing
            || session.integration.is_some()
            || session.privileged.is_some()
            || session.service.is_some()
        {
            return Ok(());
        }
        let restored = (|| -> Result<Option<Integration>> {
            let value = store.preferences("emby-environment")?;
            if value.get("web").is_some() {
                let inspection_only = value
                    .get("discovery_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let environment: product_core::emby_environment::Environment =
                    serde_json::from_value(value)?;
                return if inspection_only {
                    Integration::inspect_only(std::path::Path::new(&environment.web), &self.backup)
                } else {
                    Integration::open_accessible(
                        std::path::Path::new(&environment.web),
                        &self.backup,
                    )
                }
                .map(Some);
            }
            if !value.is_null() && value != serde_json::json!({}) {
                return Err(AppError::new(
                    "emby-environment-invalid",
                    "已保存的 Emby 目录配置无效，未自动替换",
                ));
            }
            let candidates = discover()?;
            if candidates.is_empty() {
                return Ok(None);
            }
            if candidates.len() != 1 {
                return Err(AppError::new(
                    "emby-discovery-ambiguous",
                    "检测到多个 Emby 安装目录，无法确认维护目标。",
                ));
            }
            let environment = &candidates[0];
            if environment.endpoint.is_empty() {
                return Err(
                    AppError::new("emby-discovery", environment.issues.join("；"))
                        .at(&environment.web),
                );
            }
            // Discovery never probes write access or recovers a file transaction.
            // The existing explicit maintenance action acquires writable access.
            let integration =
                Integration::inspect_only(std::path::Path::new(&environment.web), &self.backup)?;
            let mut saved = serde_json::to_value(environment)?;
            saved["discovery_only"] = serde_json::json!(true);
            store.save_preference("emby-environment", &saved)?;
            Ok(Some(integration))
        })();
        let restored = restored.and_then(|integration| {
            if let Some(value) = &integration {
                import(&value.status()?.target)?;
            }
            Ok(integration)
        });
        match restored {
            Ok(integration) => {
                session.integration = integration.map(Arc::new);
                session.restore_error = None;
            }
            Err(error) => session.restore_error = Some(error),
        }
        Ok(())
    }
    pub fn shutdown(&self) -> Result<()> {
        // v4.1.0 never abandons a file-replacement or authorization operation.
        // Keep its bounded wait so a stalled helper leaves the window open.
        self.wait_for_maintenance(Duration::from_secs(30))?;
        let mut failure = self.stop_incremental().err();
        let mut session = self
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if let Some(service) = session.service.as_mut() {
            match service.stop() {
                Ok(_) => session.service = None,
                Err(error) => append_shutdown_error(&mut failure, error),
            }
        }
        if let Some(remote) = session.privileged.as_mut() {
            match remote.shutdown() {
                Ok(()) => session.privileged = None,
                Err(error) => append_shutdown_error(&mut failure, error),
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        session.closing = true;
        Ok(())
    }
}

fn append_shutdown_error(failure: &mut Option<AppError>, next: AppError) {
    match failure {
        Some(error) => {
            error.message = format!("{}；{}: {}", error.message, next.code, next.message);
        }
        None => *failure = Some(next),
    }
}

fn begin_owned_maintenance(app: &tauri::AppHandle) -> Result<MaintenanceWork> {
    let owner = app.state::<EmbyDesktop>();
    let service = owner.service_status()?;
    owner.begin_maintenance(service, legacy_blocked(app)?)
}

pub(crate) fn legacy_blocked(app: &tauri::AppHandle) -> Result<bool> {
    let state = app.state::<EmbyDesktop>();
    let session = state
        .session
        .lock()
        .map_err(|error| AppError::new("emby-state", error))?;
    let web = session
        .integration
        .as_ref()
        .map(|integration| integration.status())
        .transpose()?
        .is_some_and(|status| status.legacy_patch.is_some());
    #[cfg(windows)]
    let system = if !app.config().identifier.ends_with(".validation") {
        let inventory = product_core::legacy_components::observe(&app.state::<Desktop>().store);
        !inventory.items.is_empty() || !inventory.errors.is_empty()
    } else {
        false
    };
    #[cfg(not(windows))]
    let system = false;
    Ok(web || system)
}

fn startup_service_ready(
    folders: &product_core::folders::FolderSettings,
    integration: Option<&ManagerIntegrationStatus>,
    legacy: &product_core::legacy_components::Review,
) -> bool {
    service_roots_ready(folders)
        && legacy.items.is_empty()
        && legacy.errors.is_empty()
        && integration
            .is_some_and(|status| !status.requires_permission && status.legacy_patch.is_none())
}

fn service_roots_ready(folders: &product_core::folders::FolderSettings) -> bool {
    folders.folders.iter().any(|folder| folder.enabled)
}

/// Restore the v4.1.0 startup behavior after Tauri has registered the shared
/// service owners. Setup remains asynchronous so a slow library scan cannot
/// delay creation of the native window.
pub fn initialize(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let store = app.state::<Desktop>().store.clone();
            let folders = tauri::async_runtime::spawn_blocking(move || store.folder_settings())
                .await
                .map_err(|error| AppError::new("folders-worker", error))??;
            if !folders.folders.iter().any(|folder| folder.enabled) {
                return Ok(());
            }
            let integration = emby_status(app.clone()).await?;
            let legacy = emby_legacy_components(None, app.clone()).await?;
            if !startup_service_ready(&folders, integration.as_ref(), &legacy) {
                return Ok(());
            }
            let id = product_core::hash(&serde_json::to_vec(&(
                "startup",
                product_core::emby::timestamp(),
            ))?);
            emby_start(id, app.clone()).await.map(|_| ())
        }
        .await;
        if let Err(error) = result {
            if let Err(record_error) = app
                .state::<EmbyDesktop>()
                .record_startup_failure(error.clone())
            {
                eprintln!("service startup status: {record_error}");
            }
            eprintln!("service startup: {error}");
        }
    });
}
fn import_original_diagnostics(app: &tauri::AppHandle, web: &str) -> Result<()> {
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    app.state::<Desktop>()
        .store
        .import_original_emby_diagnostics(
            std::env::consts::OS,
            &app.config().identifier,
            std::path::Path::new(web),
            roaming.as_deref(),
        )
        .map(|_| ())
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
#[derive(serde::Serialize)]
pub struct LegacySystemOperation {
    #[serde(flatten)]
    plan: product_core::legacy_migration::Plan,
    items: Vec<String>,
}
#[tauri::command]
pub async fn emby_legacy_operation(
    id: Option<String>,
    app: tauri::AppHandle,
) -> Result<Option<LegacySystemOperation>> {
    tauri::async_runtime::spawn_blocking(move || {
        let owner = app.state::<EmbyDesktop>();
        let _session = owner
            .session
            .lock()
            .map_err(|error| AppError::new("emby-state", error))?;
        let desktop = app.state::<Desktop>();
        let plan = match id {
            Some(id) => desktop.store.legacy_system_plan(&id)?,
            None => desktop.store.latest_legacy_system_plan()?,
        };
        plan.map(|plan| {
            Ok(LegacySystemOperation {
                items: plan.inventory.review()?.items,
                plan,
            })
        })
        .transpose()
    })
    .await
    .map_err(|error| AppError::new("emby-worker", error))?
}

#[tauri::command]
pub async fn emby_migrate_legacy_system(
    id: String,
    reviewed: String,
    app: tauri::AppHandle,
) -> Result<product_core::legacy_migration::Plan> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            if app.config().identifier.ends_with(".validation") {
                return Err(AppError::new(
                    "legacy-system-validation",
                    "验证应用不处理正式系统组件",
                ));
            }
            let owner = app.state::<EmbyDesktop>();
            let session = owner
                .session
                .lock()
                .map_err(|error| AppError::new("emby-state", error))?;
            if session.closing || session.service.is_some() {
                return Err(AppError::new(
                    "legacy-system-service-active",
                    "请先停止新版卡片服务",
                ));
            }
            if let Some(remote) = &session.privileged {
                match remote.request(product_core::maintenance::Command::Status {})? {
                    product_core::maintenance::Outcome::Status { service, .. }
                        if service.phase == "stopped" => {}
                    _ => {
                        return Err(AppError::new(
                            "legacy-system-service-active",
                            "新版卡片服务尚未确认停止",
                        ))
                    }
                }
            }
            let desktop = app.state::<Desktop>();
            let mut platform = product_core::legacy_migration::windows::Windows {
                store: &desktop.store,
            };
            product_core::legacy_migration::execute(&desktop.store, &mut platform, &id, &reviewed)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, reviewed, app);
            Err(AppError::new(
                "legacy-system-platform",
                "当前系统不适用 Windows 旧组件迁移",
            ))
        }
    })
    .await
    .map_err(|error| AppError::new("emby-worker", error))?
}

#[tauri::command]
pub async fn emby_legacy_components(
    reviewed: Option<String>,
    app: tauri::AppHandle,
) -> Result<product_core::legacy_components::Review> {
    tauri::async_runtime::spawn_blocking(move || {
        let _app = &app;
        #[cfg(windows)]
        let inventory = if !app.config().identifier.ends_with(".validation") {
            product_core::legacy_components::observe(&app.state::<Desktop>().store)
        } else {
            product_core::legacy_components::Inventory::default()
        };
        #[cfg(not(windows))]
        let inventory = product_core::legacy_components::Inventory::default();
        let report = inventory.review()?;
        if let Some(reviewed) = reviewed {
            // Read-only preflight; the separate migration command owns mutations.
            inventory.require_reviewed_clear(&reviewed)?;
        }
        Ok(report)
    })
    .await
    .map_err(|error| AppError::new("emby-worker", error))?
}

#[derive(serde::Serialize)]
pub struct ManagerIntegrationStatus {
    #[serde(flatten)]
    integration: IntegrationStatus,
    pub index_current: bool,
}
impl std::ops::Deref for ManagerIntegrationStatus {
    type Target = IntegrationStatus;
    fn deref(&self) -> &Self::Target {
        &self.integration
    }
}
fn manager_integration_status(
    store: &product_core::store::Store,
    integration: IntegrationStatus,
) -> Result<ManagerIntegrationStatus> {
    let index_current = store.index_current(
        integration
            .details
            .as_ref()
            .and_then(|details| details.data_fingerprint.as_deref()),
    )?;
    Ok(ManagerIntegrationStatus {
        integration,
        index_current,
    })
}
#[tauri::command]
pub async fn emby_status(app: tauri::AppHandle) -> Result<Option<ManagerIntegrationStatus>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        state.restore_with_import(
            &app.state::<Desktop>().store,
            || {
                let home = app
                    .path()
                    .home_dir()
                    .map_err(|e| AppError::new("home-directory", e))?;
                let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
                // v4.1.0 Windows resolves APPDATA/Emby-Server, not an arbitrary
                // second installation found in LOCALAPPDATA.
                Ok(product_core::emby_environment::discover(
                    std::env::consts::OS,
                    &home,
                    roaming.as_deref(),
                    None,
                ))
            },
            |web| import_original_diagnostics(&app, web),
        )?;
        let session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        let integration = if let Some(remote) = &session.privileged {
            match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { integration, .. } => Some(integration),
                _ => return Err(helper_reply()),
            }
        } else {
            session
                .integration
                .as_ref()
                .map(|i| i.status())
                .transpose()?
        };
        integration
            .map(|integration| {
                manager_integration_status(&app.state::<Desktop>().store, integration)
            })
            .transpose()
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub async fn emby_plan(
    id: String,
    action: String,
    reviewed: Option<String>,
    app: tauri::AppHandle,
) -> Result<MaintenancePlan> {
    tauri::async_runtime::spawn_blocking(move || {
        if (action == "adopt") != reviewed.is_some() {
            return Err(AppError::new(
                "emby-legacy-review-required",
                "迁移旧版网页卡片需要当前确认清单",
            ));
        }
        let state = app.state::<EmbyDesktop>();
        let desktop = app.state::<Desktop>();
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
        #[cfg(windows)]
        if action == "adopt"
            && !app
                .config()
                .identifier
                .to_ascii_lowercase()
                .contains("validation")
        {
            product_core::legacy_components::observe(&desktop.store).require_clear()?;
        }
        if let Some(remote) = &session.privileged {
            let index = public_catalog(&desktop.store)?;
            let command = match reviewed {
                Some(reviewed) => product_core::maintenance::Command::PlanAdoption {
                    id,
                    reviewed,
                    index,
                },
                None => product_core::maintenance::Command::Plan {
                    id,
                    action: serde_json::from_value(serde_json::Value::String(action))?,
                    index,
                },
            };
            let outcome = remote.request(command)?;
            if let product_core::maintenance::Outcome::Plan(plan) = outcome {
                desktop.store.save_preference(
                    "emby-maintenance-review",
                    &serde_json::json!({"id":plan.id,"target":plan.target,"authority":"system"}),
                )?;
                return Ok(plan);
            }
            return Err(helper_reply());
        }
        let integration = session
            .integration
            .as_ref()
            .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?;
        let plan = if let Some(reviewed) = reviewed {
            integration.plan_adoption(
                &id,
                &reviewed,
                CARD_JS,
                &public_catalog(&desktop.store)?,
                &emby::bundled_card_languages()?,
            )?
        } else {
            integration.plan(
                &id,
                &action,
                CARD_JS,
                &public_catalog(&desktop.store)?,
                &emby::bundled_card_languages()?,
            )?
        };
        desktop.store.save_preference(
            "emby-maintenance-review",
            &serde_json::json!({"id": plan.id, "target": plan.target}),
        )?;
        Ok(plan)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub async fn emby_operation(
    id: Option<String>,
    app: tauri::AppHandle,
) -> Result<Option<MaintenancePlan>> {
    tauri::async_runtime::spawn_blocking(move || {
        let latest = id.is_none();
        let state = app.state::<EmbyDesktop>();
        let desktop = app.state::<Desktop>();
        let session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if let Some(remote) = &session.privileged {
            let id = match id {
                Some(id) => id,
                None => {
                    let saved = desktop.store.preferences("emby-maintenance-review")?;
                    let environment = desktop.store.preferences("emby-environment")?;
                    if saved["target"] != environment["web"] {
                        return Ok(None);
                    }
                    let Some(id) = saved["id"].as_str() else {
                        return Ok(None);
                    };
                    id.to_owned()
                }
            };
            return match remote.request(product_core::maintenance::Command::Operation { id }) {
                Ok(product_core::maintenance::Outcome::Operation(plan)) => Ok(Some(plan)),
                Err(error) if latest && error.code == "operation-not-found" => Ok(None),
                Err(error) => Err(error),
                _ => Err(helper_reply()),
            };
        }
        let Some(integration) = session.integration.as_ref() else {
            return Ok(None);
        };
        let id = match id {
            Some(id) => id,
            None => {
                let saved = desktop.store.preferences("emby-maintenance-review")?;
                if saved["target"].as_str() != Some(integration.status()?.target.as_str()) {
                    return Ok(None);
                }
                let Some(id) = saved["id"].as_str() else {
                    return Ok(None);
                };
                if saved["authority"] == "system" {
                    return Err(AppError::new(
                        "maintenance-reconnect-required",
                        format!("Authorize this installation to query operation {id}"),
                    ));
                }
                id.to_owned()
            }
        };
        match integration.operation(&id) {
            Err(error) if latest && error.code == "operation-not-found" => Ok(None),
            result => result.map(Some),
        }
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}
#[tauri::command]
pub async fn emby_repair(id: String, app: tauri::AppHandle) -> Result<IntegrationStatus> {
    let maintenance = begin_owned_maintenance(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _maintenance = maintenance;
        let state = app.state::<EmbyDesktop>();
        let desktop = app.state::<Desktop>();
        let session = state
            .session
            .lock()
            .map_err(|error| AppError::new("emby-state", error))?;
        if let Some(remote) = &session.privileged {
            let target = match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { integration, .. } => {
                    integration.target
                }
                _ => return Err(helper_reply()),
            };
            return recorded_repair(&desktop.store, &id, &target, true, || {
                match remote
                    .request(product_core::maintenance::Command::Repair { id: id.clone() })?
                {
                    product_core::maintenance::Outcome::Applied(status) => Ok(status),
                    _ => Err(helper_reply()),
                }
            });
        }
        let target = session
            .integration
            .as_ref()
            .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?;
        let languages = emby::bundled_card_languages()?;
        recorded_repair(&desktop.store, &id, &target.status()?.target, false, || {
            if let Some(service) = &session.service {
                service.repair_web(&id, CARD_JS, &languages)
            } else {
                target.repair_web(&id, CARD_JS, &languages)
            }
        })
    })
    .await
    .map_err(|error| AppError::new("emby-worker", error))?
}
// The UI's restart pointer must be durable before the helper can mutate files.
// It records where to query the exact transaction, never claims it committed.
fn recorded_repair(
    store: &store::Store,
    id: &str,
    target: &str,
    system: bool,
    operation: impl FnOnce() -> Result<IntegrationStatus>,
) -> Result<IntegrationStatus> {
    store.save_preference(
        "emby-maintenance-review",
        &serde_json::json!({"id":id,"target":target,"action":"repair-web","authority":if system {"system"} else {"user"}}),
    )?;
    operation()
}
#[tauri::command]
pub async fn emby_apply(
    id: String,
    fingerprint: String,
    app: tauri::AppHandle,
) -> Result<IntegrationStatus> {
    let maintenance = begin_owned_maintenance(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _maintenance = maintenance;
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
        #[cfg(windows)]
        if !app
            .config()
            .identifier
            .to_ascii_lowercase()
            .contains("validation")
        {
            let plan = if let Some(remote) = &session.privileged {
                match remote
                    .request(product_core::maintenance::Command::Operation { id: id.clone() })?
                {
                    product_core::maintenance::Outcome::Operation(plan) => plan,
                    _ => return Err(helper_reply()),
                }
            } else {
                session
                    .integration
                    .as_ref()
                    .ok_or_else(|| {
                        AppError::new("emby-not-configured", "Select Emby web directory")
                    })?
                    .operation(&id)?
            };
            if plan.action == "adopt" && plan.phase != "committed" {
                product_core::legacy_components::observe(&app.state::<Desktop>().store)
                    .require_clear()?;
            }
        }
        if let Some(remote) = &session.privileged {
            return match remote
                .request(product_core::maintenance::Command::Apply { id, fingerprint })?
            {
                product_core::maintenance::Outcome::Applied(status) => Ok(status),
                _ => Err(helper_reply()),
            };
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
pub async fn scan_library(
    id: String,
    revision: u32,
    space: Space,
    app: tauri::AppHandle,
) -> Result<Vec<Task>> {
    submit_manager_scan(
        id,
        revision,
        product_core::folders::ManagerScanScope::Space(space),
        app,
    )
    .await
}
#[tauri::command]
pub async fn scan_media_folder(
    id: String,
    revision: u32,
    folder_id: String,
    app: tauri::AppHandle,
) -> Result<Vec<Task>> {
    submit_manager_scan(
        id,
        revision,
        product_core::folders::ManagerScanScope::Folder(folder_id),
        app,
    )
    .await
}
async fn submit_manager_scan(
    id: String,
    revision: u32,
    scope: product_core::folders::ManagerScanScope,
    app: tauri::AppHandle,
) -> Result<Vec<Task>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let scanner = state
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        let tasks = scanner
            .as_ref()
            .ok_or_else(|| AppError::new("service-stopped", "服务已停止，请先启动服务"))?
            .request_manager_scan(&id, revision, scope)?;
        for task in &tasks {
            if let Err(error) = app.emit("task-changed", task) {
                eprintln!("task-event: {error}");
            }
        }
        Ok(tasks)
    })
    .await
    .map_err(|e| AppError::new("incremental-worker", e))?
}
#[tauri::command]
pub async fn refresh_libraries(
    revision: u32,
    app: tauri::AppHandle,
) -> Result<product_core::incremental::IncrementalStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let scanner = state
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        scanner
            .as_ref()
            .ok_or_else(|| AppError::new("service-stopped", "服务已停止，请先启动服务"))?
            .request_refresh(revision)
    })
    .await
    .map_err(|e| AppError::new("incremental-worker", e))?
}
#[tauri::command]
pub async fn rebuild_index(id: String, revision: u32, app: tauri::AppHandle) -> Result<Vec<Task>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let scanner = state
            .scanner
            .lock()
            .map_err(|error| AppError::new("incremental-state", error))?;
        let tasks = scanner
            .as_ref()
            .ok_or_else(|| AppError::new("service-stopped", "服务已停止，请先启动服务"))?
            .request_rebuild(&id, revision)?;
        for task in &tasks {
            if let Err(error) = app.emit("task-changed", task) {
                eprintln!("task-event: {error}");
            }
        }
        Ok(tasks)
    })
    .await
    .map_err(|error| AppError::new("rebuild-worker", error))?
}
#[tauri::command]
pub async fn emby_start(id: String, app: tauri::AppHandle) -> Result<ServiceStatus> {
    let store = app.state::<Desktop>().store.clone();
    let handle = app.clone();
    let scanner_id = id.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let desktop = app.state::<Desktop>();
        if !service_roots_ready(&desktop.store.folder_settings()?) {
            return Err(AppError::new(
                "media-roots-required",
                "尚未选择媒体目录。完成设置后才能启动服务",
            ));
        }
        let mut session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.closing {
            return Err(AppError::new(
                "service-closing",
                "程序正在退出，不能启动服务",
            ));
        }
        if let Some(service) = &session.service {
            return service.status();
        }
        if let Some(remote) = &session.privileged {
            match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { service, .. } if service.active() => {
                    return Ok(service)
                }
                product_core::maintenance::Outcome::Status { .. } => {}
                _ => return Err(helper_reply()),
            }
        }
        let web = if let Some(remote) = &session.privileged {
            match remote.request(product_core::maintenance::Command::Status {})? {
                product_core::maintenance::Outcome::Status { integration, .. } => {
                    Some(integration.target)
                }
                _ => return Err(helper_reply()),
            }
        } else {
            session
                .integration
                .as_ref()
                .map(|integration| integration.status().map(|status| status.target))
                .transpose()?
        };
        if let Some(web) = web {
            import_original_diagnostics(&app, &web)?;
        }
        let status = product_core::legacy_components::start_after_check(
            || {
                #[cfg(windows)]
                if !app
                    .config()
                    .identifier
                    .to_ascii_lowercase()
                    .contains("validation")
                {
                    return Ok(product_core::legacy_components::observe(&desktop.store));
                }
                Ok(product_core::legacy_components::Inventory::default())
            },
            || {
                if let Some(remote) = &session.privileged {
                    return match remote
                        .request(product_core::maintenance::Command::Start { session: id })?
                    {
                        product_core::maintenance::Outcome::Service(status) => Ok(status),
                        _ => Err(helper_reply()),
                    };
                }
                let target = session.integration.clone().ok_or_else(|| {
                    AppError::new("emby-not-configured", "Select Emby web directory")
                })?;
                let service = CardService::start_with_store(target, &id, desktop.store.clone())?;
                let status = service.status()?;
                session.service = Some(service);
                Ok(status)
            },
        )?;
        session.initial_failure = None;
        Ok(status)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))??;
    let mut status = status;
    if status.active() {
        if let Err(error) = handle
            .state::<EmbyDesktop>()
            .start_incremental(&handle, &scanner_id)
        {
            status = handle
                .state::<EmbyDesktop>()
                .session
                .lock()
                .map_err(|e| AppError::new("emby-state", e))?
                .service_status()?;
            if status.error.is_none() {
                status.error = Some(error);
            }
        }
    }
    Ok(with_service_history(&store, status))
}
// A history write failure must retain the actual service phase and lease.
// Start, stop and status queries share this same presentation boundary.
fn with_service_history(
    store: &product_core::store::Store,
    status: ServiceStatus,
) -> ServiceStatus {
    match store.service_status_with_history(status.clone()) {
        Ok(value) => value,
        Err(error) => ServiceStatus {
            error: Some(error),
            ..status
        },
    }
}
#[tauri::command]
pub async fn emby_stop(app: tauri::AppHandle) -> Result<ServiceStatus> {
    let store = app.state::<Desktop>().store.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        state.stop_for_user()
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))??;
    Ok(with_service_history(&store, status))
}
#[tauri::command]
pub async fn emby_service_status(app: tauri::AppHandle) -> Result<ServiceStatus> {
    let store = app.state::<Desktop>().store.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        session.service_status()
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))??;
    Ok(with_service_history(&store, status))
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
fn connection_candidate(
    session: &mut Session,
    web: &str,
    backup: &std::path::Path,
) -> Result<Arc<Integration>> {
    if let Some(integration) = session.integration.as_ref() {
        let status = integration.status()?;
        if status.target == web {
            if !status.requires_permission {
                return Ok(integration.clone());
            }
            // Reopen this same installation after permission changes. Preserve
            // an unrelated connection until a different target opens successfully.
            session.integration = None;
        }
    }
    let integration = Arc::new(Integration::open_accessible(
        std::path::Path::new(web),
        backup,
    )?);
    Ok(integration)
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
    if let Some(remote) = session.privileged.as_mut() {
        match remote.request(product_core::maintenance::Command::Status {})? {
            product_core::maintenance::Outcome::Status { service, .. } if service.active() => {
                return Err(AppError::new(
                    "emby-stop-required",
                    "Stop the current card service before changing its installation",
                ))
            }
            _ => {}
        }
        remote.shutdown()?;
    }
    session.privileged = None;
    let integration = connection_candidate(&mut session, &environment.web, &state.backup)?;
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
        let owner = app.state::<EmbyDesktop>();
        let session = owner
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        session.require_running()?;
        drop(session);
        let store = &app.state::<Desktop>().store;
        let environment: product_core::emby_environment::Environment =
            serde_json::from_value(store.preferences("emby-environment")?)?;
        let data = environment.data.ok_or_else(|| {
            AppError::new("emby-data-directory", "Select Emby data directory first")
        })?;
        let receiver = owner
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?
            .as_ref()
            .ok_or_else(|| AppError::new("service-stopped", "服务已停止，请先启动服务"))?
            .request_discovery(data)?;
        receiver
            .recv()
            .map_err(|_| AppError::new("discovery-worker", "目录发现任务未返回结果"))?
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

#[cfg(test)]
mod connection_tests {
    use super::*;
    use std::fs;
    #[test]
    fn history_storage_failure_preserves_the_observed_service_state() {
        let temp = tempfile::tempdir().unwrap();
        let store = product_core::store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        for phase in ["starting", "running", "stopped", "failed"] {
            let status = ServiceStatus {
                phase: phase.into(),
                last_started_at: Some("invalid-history-time".into()),
                lease: Some(emby::Lease {
                    version: 1,
                    manager_version: "5.0.0".into(),
                    web_card_version: "5.0.0".into(),
                    session_id: "observed-session".into(),
                    sequence: 7,
                    enabled: phase == "running",
                    updated_at: "2026-10-01T01:02:03Z".into(),
                    expires_at: "2026-10-01T01:02:13Z".into(),
                }),
                error: Some(AppError::new("existing-service-error", "Original detail")),
            };
            let observed = with_service_history(&store, status.clone());
            assert_eq!(observed.phase, status.phase);
            assert_eq!(observed.last_started_at, status.last_started_at);
            assert_eq!(
                serde_json::to_value(observed.lease).unwrap(),
                serde_json::to_value(status.lease).unwrap()
            );
            assert_eq!(observed.error.unwrap().code, "emby-start-time");
        }
        let actual = ServiceStatus {
            phase: "running".into(),
            last_started_at: Some("2026-10-01T01:02:03Z".into()),
            lease: None,
            error: None,
        };
        let stored = with_service_history(&store, actual.clone());
        assert!(stored.error.is_none());
        assert_eq!(stored.last_started_at, actual.last_started_at);
        assert_eq!(stored.phase, actual.phase);
    }
    fn startup_inputs(
        enabled: bool,
        healthy: bool,
        requires_permission: bool,
        legacy: bool,
    ) -> (
        product_core::folders::FolderSettings,
        ManagerIntegrationStatus,
        product_core::legacy_components::Review,
    ) {
        (
            product_core::folders::FolderSettings {
                revision: 1,
                folders: vec![product_core::folders::MediaFolder {
                    id: "root".into(),
                    path: "/media".into(),
                    name: "media".into(),
                    kind: product_core::folders::FolderKind::Auto,
                    source: product_core::folders::FolderSource::Manual,
                    enabled,
                }],
            },
            ManagerIntegrationStatus {
                integration: IntegrationStatus {
                    target: "/emby/web".into(),
                    installed: healthy,
                    healthy,
                    phase: "disk-ready".into(),
                    issues: vec![],
                    requires_permission,
                    details: None,
                    legacy_patch: None,
                },
                index_current: false,
            },
            product_core::legacy_components::Review {
                fingerprint: "review".into(),
                items: legacy.then(|| "旧版组件".into()).into_iter().collect(),
                errors: vec![],
            },
        )
    }
    #[test]
    fn startup_matches_original_service_gate_and_allows_an_index_rebuild() {
        let (folders, integration, legacy) = startup_inputs(true, true, false, false);
        assert!(service_roots_ready(&folders));
        assert!(startup_service_ready(&folders, Some(&integration), &legacy));
        assert!(!integration.index_current);
        let (unpatched_folders, unpatched, no_legacy) = startup_inputs(true, false, false, false);
        assert!(startup_service_ready(
            &unpatched_folders,
            Some(&unpatched),
            &no_legacy
        ));
        for (enabled, healthy, permission, old) in [
            (false, true, false, false),
            (true, true, true, false),
            (true, true, false, true),
        ] {
            let (folders, integration, legacy) = startup_inputs(enabled, healthy, permission, old);
            assert_eq!(service_roots_ready(&folders), enabled);
            assert!(!startup_service_ready(
                &folders,
                Some(&integration),
                &legacy
            ));
        }
        assert!(!startup_service_ready(&folders, None, &legacy));
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let web = root.join("web");
        let backup = root.join("backups");
        fs::create_dir(&web).unwrap();
        fs::write(
            web.join("index.html"),
            format!(
                "<html><head></head><body>{}</body></html>",
                "original ".repeat(40)
            ),
        )
        .unwrap();
        (temp, web, backup)
    }
    fn waiting_session(web: &std::path::Path, backup: &std::path::Path) -> Session {
        let integration = Arc::new(Integration::open(web, backup).unwrap());
        integration
            .repair_web("setup", CARD_JS, &emby::bundled_card_languages().unwrap())
            .unwrap();
        let service = CardService::start_waiting_for_index(integration.clone(), "first").unwrap();
        Session {
            integration: Some(integration),
            service: Some(service),
            scan_session: Some("first-reader".into()),
            ..Default::default()
        }
    }
    #[test]
    fn initial_index_failure_cleans_local_owner_retains_error_and_allows_retry() {
        let (temp, web, backup) = fixture();
        let session = Arc::new(Mutex::new(waiting_session(&web, &backup)));
        let store = Arc::new(store::Store::open(&temp.path().join("state.sqlite")).unwrap());
        let complete = session.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut worker = product_core::incremental::IncrementalWorker::start_with_completion(
            store,
            "first-reader",
            || Err(AppError::new("scan-fixture", "first index failed")),
            |_| {},
            move |terminal| {
                complete
                    .lock()
                    .unwrap()
                    .fail_initial_index("first-reader", terminal.error.unwrap())
                    .unwrap();
                tx.send(()).unwrap();
            },
        )
        .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        worker.stop().unwrap();
        let mut session = session.lock().unwrap();
        for _ in 0..2 {
            let status = session.service_status().unwrap();
            assert_eq!(status.phase, "error");
            assert_eq!(status.error.unwrap().code, "scan-fixture");
            assert!(!status.lease.unwrap().enabled);
            assert!(status.last_started_at.is_none());
        }
        assert!(session.service.is_none());
        session.service = Some(
            CardService::start_waiting_for_index(session.integration.clone().unwrap(), "retry")
                .unwrap(),
        );
        session.initial_failure = None;
        session.scan_session = Some("retry-reader".into());
        session
            .fail_initial_index("first-reader", AppError::new("late", "old reader"))
            .unwrap();
        assert_eq!(session.service_status().unwrap().phase, "starting");
        assert_eq!(
            session.service_status().unwrap().lease.unwrap().session_id,
            "retry"
        );
        session.stop_service().unwrap();
        assert_eq!(session.service_status().unwrap().phase, "stopped");
    }
    #[test]
    fn initial_index_failure_ignores_explicit_stop_exit_and_running_service() {
        for closing in [false, true] {
            let (_temp, web, backup) = fixture();
            let mut session = waiting_session(&web, &backup);
            session.closing = closing;
            if !closing {
                session.scan_session = None;
            }
            session
                .fail_initial_index("first-reader", AppError::new("late", "cancelled"))
                .unwrap();
            assert!(session.initial_failure.is_none());
            session.stop_service().unwrap();
        }
        let (_temp, web, backup) = fixture();
        let mut session = waiting_session(&web, &backup);
        session.stop_service().unwrap();
        session.service =
            Some(CardService::start(session.integration.clone().unwrap(), "running").unwrap());
        assert_eq!(session.service_status().unwrap().phase, "running");
        session
            .fail_initial_index(
                "first-reader",
                AppError::new("later-read", "not an initial failure"),
            )
            .unwrap();
        assert_eq!(session.service_status().unwrap().phase, "running");
        session.stop_service().unwrap();
    }
    #[test]
    fn initial_index_failure_shutdown_joins_a_reader_with_a_pending_callback() {
        let (temp, web, backup) = fixture();
        let owner = Arc::new(EmbyDesktop::new(backup.clone()));
        *owner.session.lock().unwrap() = waiting_session(&web, &backup);
        let store = Arc::new(store::Store::open(&temp.path().join("state.sqlite")).unwrap());
        let (entered, ready) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let blocked = Mutex::new(blocked);
        let completion = owner.clone();
        *owner.scanner.lock().unwrap() = Some(
            product_core::incremental::IncrementalWorker::start_with_completion(
                store,
                "first-reader",
                move || {
                    entered.send(()).unwrap();
                    blocked.lock().unwrap().recv().unwrap();
                    Err(AppError::new("late", "read failed while exiting"))
                },
                |_| {},
                move |terminal| {
                    completion
                        .session
                        .lock()
                        .unwrap()
                        .fail_initial_index("first-reader", terminal.error.unwrap())
                        .unwrap();
                },
            )
            .unwrap(),
        );
        ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let exiting = owner.clone();
        let (done, finished) = std::sync::mpsc::channel();
        let exit = std::thread::spawn(move || {
            done.send(exiting.shutdown()).unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while owner.session.lock().unwrap().scan_session.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "shutdown did not cancel the reader identity"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        release.send(()).unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        exit.join().unwrap();
        owner.shutdown().unwrap();
        let session = owner.session.lock().unwrap();
        assert!(session.closing);
        assert!(session.initial_failure.is_none());
        assert!(session.service.is_none());
        assert_eq!(
            owner
                .scanner
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .status()
                .unwrap()
                .phase,
            "failed"
        );
    }

    #[test]
    fn shutdown_revokes_the_card_service_after_scanner_owner_failure() {
        let (_temp, web, backup) = fixture();
        let owner = Arc::new(EmbyDesktop::new(backup.clone()));
        *owner.session.lock().unwrap() = waiting_session(&web, &backup);
        let poisoned = owner.clone();
        assert!(std::thread::spawn(move || {
            let _scanner = poisoned.scanner.lock().unwrap();
            panic!("poison scanner owner");
        })
        .join()
        .is_err());

        assert_eq!(owner.shutdown().unwrap_err().code, "incremental-state");
        let session = owner.session.lock().unwrap();
        assert!(session.service.is_none());
        assert!(!session.closing);
    }

    #[test]
    fn visible_stop_revokes_the_card_service_and_retains_scanner_failure_for_retry() {
        let (_temp, web, backup) = fixture();
        let owner = Arc::new(EmbyDesktop::new(backup.clone()));
        *owner.session.lock().unwrap() = waiting_session(&web, &backup);
        let poisoned = owner.clone();
        assert!(std::thread::spawn(move || {
            let _scanner = poisoned.scanner.lock().unwrap();
            panic!("poison scanner owner");
        })
        .join()
        .is_err());

        let status = owner.stop_for_user().unwrap();
        assert_eq!(status.phase, "failed");
        assert_eq!(status.error.unwrap().code, "incremental-state");
        let session = owner.session.lock().unwrap();
        assert!(session.service.is_none());
        assert_eq!(session.initial_failure.as_ref().unwrap().phase, "failed");
        assert!(!session.closing);
    }

    #[test]
    fn maintenance_exit_wait_is_bounded_and_retryable() {
        let owner = EmbyDesktop::new(PathBuf::from("unused"));
        let first = owner
            .begin_maintenance(
                ServiceStatus {
                    phase: "running".into(),
                    last_started_at: None,
                    lease: None,
                    error: None,
                },
                true,
            )
            .unwrap();
        let duplicate = owner.begin_maintenance(
            ServiceStatus {
                phase: "stopped".into(),
                last_started_at: None,
                lease: None,
                error: None,
            },
            false,
        );
        assert!(matches!(duplicate, Err(error) if error.code == "task-busy"));
        let context = owner.maintenance_context().unwrap().unwrap();
        assert_eq!(context.phase, "running");
        assert!(context.legacy_blocked);
        assert_eq!(
            owner
                .wait_for_maintenance(Duration::from_millis(5))
                .unwrap_err()
                .code,
            "maintenance-exit-timeout"
        );
        drop(first);
        owner
            .wait_for_maintenance(Duration::from_millis(5))
            .unwrap();
        assert!(owner.maintenance_context().unwrap().is_none());
    }
    #[test]
    fn initial_index_failure_before_reader_spawn_still_cleans_the_service() {
        let (temp, web, backup) = fixture();
        let mut session = waiting_session(&web, &backup);
        let store = Arc::new(store::Store::open(&temp.path().join("state.sqlite")).unwrap());
        let error = match product_core::incremental::IncrementalWorker::start_with_completion(
            store,
            "invalid reader identity",
            || Ok(true),
            |_| {},
            |_| panic!("no reader was created"),
        ) {
            Err(error) => error,
            Ok(_) => panic!("invalid identity must reject before spawn"),
        };
        session.fail_initial_index("first-reader", error).unwrap();
        let status = session.service_status().unwrap();
        assert_eq!(status.phase, "error");
        assert_eq!(status.error.unwrap().code, "invalid-operation-id");
        assert!(session.service.is_none());
    }
    #[test]
    fn initial_index_failure_keeps_cleanup_failure_actionable() {
        let (_temp, web, backup) = fixture();
        let mut session = waiting_session(&web, &backup);
        let initial = session.service_status().unwrap();
        for cleanup in [
            Err(AppError::new("stop-denied", "denied")),
            Ok(initial.clone()),
        ] {
            let status = initial_index_failure(
                initial.clone(),
                AppError::new("scan-fixture", "failed"),
                || cleanup,
            );
            assert_eq!(status.phase, "failed");
            let error = status.error.unwrap();
            assert_eq!(error.code, "scan-fixture");
            assert!(error.message.contains("停止服务"));
        }
        session.stop_service().unwrap();
    }
    #[test]
    fn manager_status_distinguishes_complete_files_from_current_library_contents() {
        let (temp, web, backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let integration = Integration::open(&web, &backup).unwrap();
        let plan = integration
            .plan(
                "install",
                "install",
                CARD_JS,
                &emby::public_index(&[], emby::timestamp()),
                &emby::bundled_card_languages().unwrap(),
            )
            .unwrap();
        integration.apply(&plan.id, &plan.fingerprint).unwrap();
        let observed = manager_integration_status(&store, integration.status().unwrap()).unwrap();
        assert!(observed.healthy);
        assert!(!observed.index_current);
        let media = temp.path().canonicalize().unwrap().join("media");
        fs::create_dir(&media).unwrap();
        store
            .configure(
                "roots",
                Configuration {
                    roots: vec![LibraryRoot {
                        id: "movie".into(),
                        space: Space::Movie,
                        path: media.to_str().unwrap().into(),
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .submit(ScanRequest {
                operation_id: "empty-scan".into(),
                space: Space::Movie,
                root_ids: vec!["movie".into()],
            })
            .unwrap();
        store.run_next(|| false, |_| {}).unwrap();
        let observed = manager_integration_status(&store, integration.status().unwrap()).unwrap();
        assert!(observed.index_current);
        let wire = serde_json::to_value(&observed).unwrap();
        assert_eq!(wire["target"], web.to_str().unwrap());
        assert_eq!(wire["healthy"], true);
        assert_eq!(wire["index_current"], true);
        assert!(wire.get("integration").is_none());
        let other = temp.path().canonicalize().unwrap().join("other");
        fs::create_dir(&other).unwrap();
        let mut config = store.configuration().unwrap();
        config.roots[0].path = other.to_str().unwrap().into();
        store.configure("change-root", config).unwrap();
        let observed = manager_integration_status(&store, integration.status().unwrap()).unwrap();
        assert!(observed.healthy);
        assert!(!observed.index_current);
    }
    #[test]
    fn repair_reply_loss_keeps_a_durable_restart_pointer_before_the_effect() {
        for system in [false, true] {
            let (temp, web, backup) = fixture();
            let database = temp.path().join("state.sqlite");
            let store = store::Store::open(&database).unwrap();
            let integration = Integration::open(&web, &backup).unwrap();
            let target = integration.status().unwrap().target;
            let error = recorded_repair(&store, "repair-reply-loss", &target, system, || {
                let saved = store.preferences("emby-maintenance-review")?;
                assert_eq!(saved["id"], "repair-reply-loss");
                assert_eq!(saved["target"], target);
                assert_eq!(saved["authority"], if system { "system" } else { "user" });
                integration.repair_web(
                    "repair-reply-loss",
                    CARD_JS,
                    &emby::bundled_card_languages()?,
                )?;
                Err(AppError::new(
                    "reply-lost",
                    "simulated transport interruption after commit",
                ))
            })
            .unwrap_err();
            assert_eq!(error.code, "reply-lost");
            let script = fs::read(web.join("technical-specs-card.js")).unwrap();
            drop(integration);
            drop(store);
            let store = store::Store::open(&database).unwrap();
            let saved = store.preferences("emby-maintenance-review").unwrap();
            let integration = Integration::open(&web, &backup).unwrap();
            let receipt = integration
                .operation(saved["id"].as_str().unwrap())
                .unwrap();
            assert_eq!(receipt.phase, "committed");
            assert_eq!(receipt.action, "repair-web");
            assert_eq!(receipt.target, target);
            assert_eq!(
                fs::read(web.join("technical-specs-card.js")).unwrap(),
                script
            );
        }
    }
    #[test]
    fn unavailable_restart_journal_prevents_repair_from_starting() {
        let (temp, web, _backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let before = fs::read(web.join("index.html")).unwrap();
        store.freeze_for_update().unwrap();
        assert!(
            recorded_repair(&store, "repair", web.to_str().unwrap(), true, || panic!(
                "must not dispatch repair before its pointer is durable"
            ))
            .is_err()
        );
        assert_eq!(
            store.preferences("emby-maintenance-review").unwrap(),
            serde_json::json!({})
        );
        assert_eq!(fs::read(web.join("index.html")).unwrap(), before);
    }
    #[test]
    fn discovery_requires_a_running_service_and_rejects_shutdown() {
        let session = Session::default();
        assert_eq!(
            session.require_running().unwrap_err().code,
            "service-stopped"
        );
        let closing = Session {
            closing: true,
            ..Default::default()
        };
        assert_eq!(
            closing.require_running().unwrap_err().code,
            "service-stopped"
        );
    }
    #[test]
    fn original_state_import_failure_remains_retryable_before_connecting_or_starting() {
        let (temp, web, backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let state = EmbyDesktop::new(backup);
        state
            .restore_with_import(
                &store,
                || Ok(vec![product_core::emby_environment::inspect(&web)?]),
                |observed| {
                    assert_eq!(observed, web.to_str().unwrap());
                    Err(AppError::new(
                        "migration-source-unverified",
                        "original worker changed",
                    ))
                },
            )
            .unwrap();
        {
            let session = state.session.lock().unwrap();
            assert!(session.integration.is_none());
            assert!(session.service.is_none());
            assert_eq!(
                session.restore_error.as_ref().unwrap().code,
                "migration-source-unverified"
            );
        }
        state
            .restore_with_import(&store, || panic!("saved installation must win"), |_| Ok(()))
            .unwrap();
        assert!(state.session.lock().unwrap().integration.is_some());
        assert!(state.session.lock().unwrap().restore_error.is_none());
        state
            .restore_with_import(
                &store,
                || panic!("must not rediscover"),
                |_| panic!("must not reimport each status poll"),
            )
            .unwrap();
        state.shutdown().unwrap();
    }
    #[test]
    fn first_discovery_and_restart_are_readonly_and_do_not_start_or_maintain_emby() {
        let (temp, web, backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let html = fs::read(web.join("index.html")).unwrap();
        let modified = fs::metadata(web.join("index.html"))
            .unwrap()
            .modified()
            .unwrap();
        let state = EmbyDesktop::new(backup.clone());
        state
            .restore(&store, || {
                Ok(vec![product_core::emby_environment::inspect(&web)?])
            })
            .unwrap();
        {
            let session = state.session.lock().unwrap();
            assert!(
                session
                    .integration
                    .as_ref()
                    .unwrap()
                    .status()
                    .unwrap()
                    .requires_permission
            );
            assert!(session.service.is_none());
            assert!(session.privileged.is_none());
        }
        state
            .restore(&store, || panic!("connected target must not rediscover"))
            .unwrap();
        assert_eq!(
            store.preferences("emby-environment").unwrap()["discovery_only"],
            true
        );
        drop(state);
        let state = EmbyDesktop::new(backup);
        state
            .restore(&store, || {
                panic!("saved discovery must not choose a new target")
            })
            .unwrap();
        assert!(
            state
                .session
                .lock()
                .unwrap()
                .integration
                .as_ref()
                .unwrap()
                .status()
                .unwrap()
                .requires_permission
        );
        assert_eq!(fs::read(web.join("index.html")).unwrap(), html);
        assert_eq!(
            fs::metadata(web.join("index.html"))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        assert_eq!(fs::read_dir(&web).unwrap().count(), 1);
    }
    #[test]
    fn missing_saved_target_and_ambiguous_discovery_never_choose_another_installation() {
        let (temp, web, backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let environment = product_core::emby_environment::inspect(&web).unwrap();
        let state = EmbyDesktop::new(backup);
        state
            .restore(&store, || {
                Ok(vec![environment.clone(), environment.clone()])
            })
            .unwrap();
        assert!(state.session.lock().unwrap().integration.is_none());
        assert_eq!(
            state
                .session
                .lock()
                .unwrap()
                .restore_error
                .as_ref()
                .unwrap()
                .code,
            "emby-discovery-ambiguous"
        );
        assert_eq!(
            store.preferences("emby-environment").unwrap(),
            serde_json::json!({})
        );
        let mut saved = serde_json::to_value(&environment).unwrap();
        saved["web"] = serde_json::json!(web.join("missing"));
        store.save_preference("emby-environment", &saved).unwrap();
        state
            .restore(&store, || panic!("missing saved target must not fall back"))
            .unwrap();
        assert!(state.session.lock().unwrap().integration.is_none());
        assert!(state.session.lock().unwrap().restore_error.is_some());
        assert_eq!(store.preferences("emby-environment").unwrap(), saved);
        store
            .save_preference("emby-environment", &serde_json::json!({"invalid":true}))
            .unwrap();
        state
            .restore(&store, || {
                panic!("malformed saved target must not be overwritten")
            })
            .unwrap();
        assert_eq!(
            state
                .session
                .lock()
                .unwrap()
                .restore_error
                .as_ref()
                .unwrap()
                .code,
            "emby-environment-invalid"
        );
    }
    #[test]
    fn discovery_can_retry_after_installation_but_shutdown_prevents_late_reopening() {
        let (temp, web, backup) = fixture();
        let store = store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let state = EmbyDesktop::new(backup);
        state.restore(&store, || Ok(vec![])).unwrap();
        assert!(state.session.lock().unwrap().integration.is_none());
        state
            .restore(&store, || {
                Ok(vec![product_core::emby_environment::inspect(&web)?])
            })
            .unwrap();
        assert!(state.session.lock().unwrap().integration.is_some());
        state.shutdown().unwrap();
        state.session.lock().unwrap().integration = None;
        state
            .restore(&store, || panic!("shutdown must not rediscover"))
            .unwrap();
        assert!(state.session.lock().unwrap().integration.is_none());
    }
    #[test]
    fn failed_new_target_keeps_original_session_and_its_exclusive_lock() {
        let (_temp, web, backup) = fixture();
        let mut session = Session {
            integration: Some(Arc::new(Integration::open(&web, &backup).unwrap())),
            ..Default::default()
        };
        let bad = web.with_file_name("invalid-target");
        fs::create_dir(&bad).unwrap();
        fs::write(bad.join("index.html"), "invalid").unwrap();
        assert!(connection_candidate(&mut session, bad.to_str().unwrap(), &backup).is_err());
        assert_eq!(
            session
                .integration
                .as_ref()
                .unwrap()
                .status()
                .unwrap()
                .target,
            web.to_string_lossy()
        );
        assert_eq!(
            Integration::open(&web, &backup.with_file_name("other-backups"))
                .err()
                .unwrap()
                .code,
            "emby-busy"
        );
    }
    #[test]
    fn permission_recheck_releases_only_the_same_readonly_connection() {
        let (_temp, web, backup) = fixture();
        let mut session = Session {
            integration: Some(Arc::new(Integration::inspect_only(&web, &backup).unwrap())),
            ..Default::default()
        };
        let next = connection_candidate(&mut session, web.to_str().unwrap(), &backup).unwrap();
        assert!(!next.status().unwrap().requires_permission);
        session.integration = Some(next.clone());
        assert!(Arc::ptr_eq(
            &next,
            &connection_candidate(&mut session, web.to_str().unwrap(), &backup).unwrap()
        ));
        assert!(!next.status().unwrap().installed);
    }
}

fn helper_reply() -> AppError {
    AppError::new("maintenance-protocol", "Unexpected helper response")
}
#[tauri::command]
pub fn emby_authorization_available() -> bool {
    crate::privileged::available()
}
#[tauri::command]
pub async fn emby_authorize(app: tauri::AppHandle) -> Result<IntegrationStatus> {
    let maintenance = begin_owned_maintenance(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _maintenance = maintenance;
        let state = app.state::<EmbyDesktop>();
        let desktop = app.state::<Desktop>();
        let mut session = state
            .session
            .lock()
            .map_err(|e| AppError::new("emby-state", e))?;
        if session.service.is_some() {
            return Err(AppError::new(
                "emby-stop-required",
                "Stop the service before authorization",
            ));
        }
        if let Some(remote) = session.privileged.as_mut() {
            remote.shutdown()?;
        }
        session.privileged = None;
        let environment = desktop.store.preferences("emby-environment")?;
        let web = environment["web"]
            .as_str()
            .ok_or_else(|| AppError::new("emby-not-configured", "Select Emby web directory"))?;
        let local = connection_candidate(&mut session, web, &state.backup)?;
        let local_status = local.status()?;
        session.integration = Some(local);
        if !local_status.requires_permission {
            let mut saved = environment.clone();
            saved
                .as_object_mut()
                .ok_or_else(helper_reply)?
                .remove("discovery_only");
            desktop.store.save_preference("emby-environment", &saved)?;
            session.restore_error = None;
            return Ok(local_status);
        }
        let remote = crate::privileged::launch(std::path::Path::new(web), desktop.store.clone())?;
        let status = match remote.request(product_core::maintenance::Command::Status {})? {
            product_core::maintenance::Outcome::Status { integration, .. } => integration,
            _ => return Err(helper_reply()),
        };
        // User-owned plans are never copied to the privileged journal. The UI
        // obtains a fresh plan and confirmation after this authorization.
        let prior = desktop.store.preferences("emby-maintenance-review")?;
        if prior["authority"] != "system" || prior["target"] != web {
            desktop
                .store
                .save_preference("emby-maintenance-review", &serde_json::json!({}))?;
        }
        session.privileged = Some(remote);
        session.restore_error = None;
        Ok(status)
    })
    .await
    .map_err(|e| AppError::new("emby-worker", e))?
}

#[tauri::command]
pub async fn incremental_status(
    app: tauri::AppHandle,
) -> Result<product_core::incremental::IncrementalStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<EmbyDesktop>();
        let scanner = state
            .scanner
            .lock()
            .map_err(|e| AppError::new("incremental-state", e))?;
        scanner
            .as_ref()
            .map(|worker| worker.status())
            .unwrap_or_else(|| Ok(Default::default()))
    })
    .await
    .map_err(|e| AppError::new("incremental-worker", e))?
}
#[tauri::command]
pub async fn incremental_settings(
    app: tauri::AppHandle,
) -> Result<product_core::incremental::IncrementalSettings> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Desktop>().store.incremental_settings()
    })
    .await
    .map_err(|e| AppError::new("incremental-worker", e))?
}
#[tauri::command]
pub async fn save_incremental_settings(
    id: String,
    value: product_core::incremental::IncrementalSettings,
    app: tauri::AppHandle,
) -> Result<product_core::incremental::IncrementalSettings> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Desktop>()
            .store
            .save_incremental_settings(&id, value)
    })
    .await
    .map_err(|e| AppError::new("incremental-worker", e))?
}
