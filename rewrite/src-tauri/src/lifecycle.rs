use crate::desktop::Desktop;
use product_core::{
    lifecycle::{self, Autostart, CloseAction, Settings, SettingsOperation},
    *,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};
#[cfg(windows)]
use tauri_plugin_autostart::ManagerExt;
struct Native<'a>(&'a tauri::AppHandle);
#[cfg(not(windows))]
impl Native<'_> {
    fn file(&self) -> Result<product_core::startup_file::StartupFile> {
        #[cfg(target_os = "macos")]
        let directory = self
            .0
            .path()
            .home_dir()
            .map_err(|e| AppError::new("autostart-directory", e))?
            .join("Library/LaunchAgents");
        #[cfg(target_os = "linux")]
        let directory = self
            .0
            .path()
            .config_dir()
            .map_err(|e| AppError::new("autostart-directory", e))?
            .join("autostart");
        let mut exe = std::env::current_exe().map_err(|e| AppError::new("autostart-path", e))?;
        #[cfg(target_os = "linux")]
        if let Some(path) = self.0.env().appimage {
            exe = path.into();
        }
        #[cfg(not(target_os = "linux"))]
        let _ = &mut exe;
        product_core::startup_file::StartupFile::new(
            std::env::consts::OS,
            &directory,
            &self.0.config().identifier,
            &exe,
            &self.0.package_info().name,
        )
    }
}
impl Autostart for Native<'_> {
    fn enabled(&self) -> Result<bool> {
        #[cfg(windows)]
        {
            self.0
                .autolaunch()
                .is_enabled()
                .map_err(|e| AppError::new("autostart-read", e))
        }
        #[cfg(not(windows))]
        {
            self.file()?.enabled()
        }
    }
    fn set(&self, enabled: bool) -> Result<()> {
        #[cfg(windows)]
        {
            let manager = self.0.autolaunch();
            if enabled {
                manager.enable()
            } else {
                manager.disable()
            }
            .map_err(|e| AppError::new("autostart-change", e))
        }
        #[cfg(not(windows))]
        {
            self.file()?.set(enabled)
        }
    }
}
#[derive(Default)]
pub struct Lifecycle {
    gate: Mutex<()>,
    pub allow_exit: AtomicBool,
    closing: AtomicBool,
    pub tray: AtomicBool,
    error: Mutex<Option<AppError>>,
}
#[derive(Serialize)]
pub struct LifecycleStatus {
    settings: Settings,
    native_autostart: Option<bool>,
    background_mode: String,
    tray_available: bool,
    closing: bool,
    error: Option<AppError>,
}
pub fn initialize(app: &tauri::AppHandle) {
    let result = lifecycle::reconcile(&app.state::<Desktop>().store, &Native(app));
    if let Err(error) = result {
        set_error(app, error);
    }
}
fn set_error(app: &tauri::AppHandle, error: AppError) {
    if let Ok(mut current) = app.state::<Lifecycle>().error.lock() {
        *current = Some(error.clone());
    }
    let _ = app.emit("lifecycle-error", error);
}
#[tauri::command]
pub fn lifecycle_status(app: tauri::AppHandle) -> Result<LifecycleStatus> {
    let state = app.state::<Lifecycle>();
    let settings = app.state::<Desktop>().store.lifecycle_settings()?;
    let native = Native(&app).enabled();
    let mut error = state
        .error
        .lock()
        .map_err(|e| AppError::new("lifecycle-state", e))?
        .clone();
    if let Err(e) = &native {
        error = Some(e.clone());
    }
    Ok(LifecycleStatus {
        settings,
        native_autostart: native.ok(),
        background_mode: if cfg!(target_os = "linux") {
            "minimize"
        } else {
            "hide"
        }
        .into(),
        tray_available: state.tray.load(Ordering::SeqCst),
        closing: state.closing.load(Ordering::SeqCst),
        error,
    })
}
#[tauri::command]
pub async fn lifecycle_apply(
    id: String,
    settings: Settings,
    app: tauri::AppHandle,
) -> Result<SettingsOperation> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Lifecycle>();
        let _guard = state
            .gate
            .lock()
            .map_err(|e| AppError::new("lifecycle-state", e))?;
        lifecycle::reconcile(&app.state::<Desktop>().store, &Native(&app))?;
        let value = lifecycle::apply(&app.state::<Desktop>().store, &id, settings, &Native(&app))?;
        *state
            .error
            .lock()
            .map_err(|e| AppError::new("lifecycle-state", e))? = None;
        Ok(value)
    })
    .await
    .map_err(|e| AppError::new("lifecycle-worker", e))?
}
#[tauri::command]
pub fn background_window(app: tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::new("window-unavailable", "Main window is unavailable"))?;
    // Linux desktops can accept a tray item without displaying it. Keep a taskbar
    // window as the guaranteed equivalent restore entry on every Linux desktop.
    if cfg!(target_os = "linux") {
        window.minimize()
    } else {
        window.hide()
    }
    .map_err(|e| AppError::new("window-background", e))
}
pub fn first_window(app: &tauri::AppHandle) -> Result<()> {
    let settings = app.state::<Desktop>().store.lifecycle_settings()?;
    if settings.start_hidden && std::env::args().any(|a| a == "--background") {
        background_window(app.clone())?;
    }
    Ok(())
}
pub fn close_requested(app: &tauri::AppHandle) {
    match app.state::<Desktop>().store.lifecycle_settings() {
        Ok(settings) if settings.close_action == CloseAction::Background => {
            if let Err(error) = background_window(app.clone()) {
                set_error(app, error);
                crate::restore(app);
            }
        }
        Ok(_) => request_exit(app),
        Err(error) => {
            set_error(app, error);
            crate::restore(app);
        }
    }
}
pub fn request_exit(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    if state.closing.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let _ = app.emit("lifecycle-closing", true);
    tauri::async_runtime::spawn_blocking(move || match crate::prepare_update_exit(&handle) {
        Ok(()) => {
            handle
                .state::<Lifecycle>()
                .allow_exit
                .store(true, Ordering::SeqCst);
            handle.exit(0);
        }
        Err(error) => {
            let state = handle.state::<Lifecycle>();
            state.closing.store(false, Ordering::SeqCst);
            state.allow_exit.store(false, Ordering::SeqCst);
            set_error(&handle, error);
            crate::restore(&handle);
        }
    });
}
