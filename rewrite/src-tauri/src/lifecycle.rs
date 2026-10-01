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
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
struct Native<'a>(&'a tauri::AppHandle);
#[cfg(windows)]
impl Native<'_> {
    fn registration(&self) -> Result<product_core::windows_startup::LoginRegistration> {
        let exe = std::env::current_exe().map_err(|e| AppError::new("autostart-path", e))?;
        product_core::windows_startup::LoginRegistration::new(&self.0.package_info().name, &exe)
    }
}
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
            self.registration()?.enabled()
        }
        #[cfg(not(windows))]
        {
            self.file()?.enabled()
        }
    }
    fn set(&self, enabled: bool) -> Result<()> {
        #[cfg(windows)]
        {
            self.registration()?.set(enabled)
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
    prompting: AtomicBool,
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
fn show_shutdown_error(app: &tauri::AppHandle, error: &AppError) {
    let localized =
        crate::languages::snapshot(app, &app.state::<crate::languages::Languages>()).ok();
    let text = localized
        .as_ref()
        .and_then(|snapshot| {
            crate::languages::native_message(
                snapshot,
                "服务、界面或所属进程没有完全停止，程序将保持运行以便重试。",
                "The service, window, or owned processes did not stop completely. The app will remain open so you can retry.",
            )
            .ok()
        })
        .unwrap_or_else(|| {
            "服务、界面或所属进程没有完全停止，程序将保持运行以便重试。\n\nThe service, window, or owned processes did not stop completely. The app will remain open so you can retry.".into()
        });
    let title = localized
        .as_ref()
        .and_then(|snapshot| {
            crate::languages::native_message(
                snapshot,
                "Tech Card Manager 退出未完成",
                "Tech Card Manager did not exit completely",
            )
            .ok()
        })
        .unwrap_or_else(|| "Tech Card Manager 退出未完成 / Exit Incomplete".into());
    let mut dialog = app
        .dialog()
        .message(format!(
            "{text}\n\n{}",
            crate::languages::backend_message(app, &error.message)
        ))
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.blocking_show();
}
#[tauri::command]
pub fn lifecycle_status(app: tauri::AppHandle) -> Result<LifecycleStatus> {
    let state = app.state::<Lifecycle>();
    let native = Native(&app).enabled();
    let settings = settings_for_presentation(&app.state::<Desktop>().store, &native)?;
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
fn settings_for_presentation(
    store: &product_core::store::Store,
    native: &Result<bool>,
) -> Result<Settings> {
    let mut settings = store.lifecycle_settings()?;
    // TCM v4.1.0 always exits from the window close action. Keep reading the
    // old rewrite-only field for data compatibility, but never expose or apply
    // its extra background-on-close behavior.
    settings.close_action = CloseAction::Quit;
    if let Ok(enabled) = native {
        settings.launch_at_login = *enabled;
    }
    Ok(settings)
}
#[tauri::command]
pub async fn lifecycle_apply(
    id: String,
    mut settings: Settings,
    app: tauri::AppHandle,
) -> Result<SettingsOperation> {
    tauri::async_runtime::spawn_blocking(move || {
        settings.close_action = CloseAction::Quit;
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
    let tray = app.state::<Lifecycle>().tray.load(Ordering::SeqCst);
    // Linux desktops can accept a tray item without displaying it. Windows and
    // macOS may also fail to create one. Keep a system-managed minimized window
    // whenever a verified tray restore entry is unavailable.
    if background_hides(tray) {
        window.hide()
    } else {
        window.minimize()
    }
    .map_err(|e| AppError::new("window-background", e))
}
fn background_hides(tray: bool) -> bool {
    !cfg!(target_os = "linux") && tray
}
pub fn first_window(app: &tauri::AppHandle) -> Result<()> {
    let settings = app.state::<Desktop>().store.lifecycle_settings()?;
    if lifecycle::silent_login_launch(&settings, std::env::args()) {
        background_window(app.clone())?;
    }
    Ok(())
}
pub fn close_requested(app: &tauri::AppHandle) {
    confirm_exit(app);
}

struct ExitContext {
    phase: String,
    legacy_blocked: bool,
    job_running: bool,
    maintenance_running: bool,
}

fn exit_message(
    snapshot: &product_core::languages::LanguageSnapshot,
    context: &ExitContext,
) -> Result<String> {
    let (chinese, english) = if context.legacy_blocked {
        (
            "当前新版服务尚未启动。退出不会修改检测到的旧版组件；旧版仍可能继续控制 Emby 技术规格卡片。",
            "The new service has not started. Exiting will not change detected legacy components; a legacy component may continue to control the Emby Technical Specs card.",
        )
    } else if context.phase == "failed" {
        (
            "上一次停止未能确认 Emby 撤卡状态。程序会再次尝试停止；如果仍失败，将保留界面并显示错误，不会伪装成已经退出。",
            "The last stop attempt could not confirm that the card was removed from Emby. The app will try again; if it still fails, the window will remain open and show the error.",
        )
    } else if !matches!(context.phase.as_str(), "starting" | "running") {
        (
            "Tech Card Manager 服务已经停止。退出后将关闭管理界面，Emby 不会显示由新版服务提供的技术规格卡片。",
            "The Tech Card Manager service is already stopped. Exiting will close the Manager, and Emby will not show a Technical Specs card supplied by the new service.",
        )
    } else {
        (
            "即将退出 Tech Card Manager。退出后将停止全部服务，并从当前打开的 Emby 页面撤下技术规格卡片。媒体文件和 NFO 不会被修改。",
            "Tech Card Manager is about to exit. All services will stop and the Technical Specs card will be removed from currently open Emby pages. Media files and NFO files will not be changed.",
        )
    };
    let mut message = crate::languages::native_message(snapshot, chinese, english)?;
    if context.maintenance_running {
        message.push_str(&crate::languages::native_message(
            snapshot,
            "\n\n管理员维护事务正在执行。确认退出后，程序会先等待该事务安全结束，不会把提权进程留在后台。",
            "\n\nAn administrator maintenance transaction is running. After you confirm, the app will wait for it to finish safely and will not leave an elevated process behind.",
        )?);
    } else if context.job_running {
        message.push_str(&crate::languages::native_message(
            snapshot,
            "\n\n当前媒体库检查将被安全取消。",
            "\n\nThe current media-library check will be cancelled safely.",
        )?);
    }
    message.push_str(&crate::languages::native_message(
        snapshot,
        "\n\n是否退出？",
        "\n\nExit now?",
    )?);
    Ok(message)
}

#[cfg(windows)]
fn exit_dialog(app: &tauri::AppHandle, title: &str, message: &str) -> bool {
    use std::ffi::c_void;
    unsafe extern "system" {
        fn MessageBoxW(owner: *mut c_void, text: *const u16, caption: *const u16, kind: u32)
            -> i32;
    }
    const MB_OK_CANCEL: u32 = 0x0000_0001;
    const MB_ICON_WARNING: u32 = 0x0000_0030;
    const MB_DEFAULT_BUTTON_2: u32 = 0x0000_0100;
    const MB_SET_FOREGROUND: u32 = 0x0001_0000;
    const MB_TOPMOST: u32 = 0x0004_0000;
    const ID_OK: i32 = 1;
    let owner = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map(|handle| handle.0)
        .unwrap_or(std::ptr::null_mut());
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // Match the v4.1.0 Win32 prompt: the owned warning is foreground/topmost,
    // and Cancel is the default so Enter cannot accidentally exit the Manager.
    unsafe {
        MessageBoxW(
            owner,
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK_CANCEL | MB_ICON_WARNING | MB_DEFAULT_BUTTON_2 | MB_SET_FOREGROUND | MB_TOPMOST,
        ) == ID_OK
    }
}

#[cfg(not(windows))]
fn exit_dialog(app: &tauri::AppHandle, title: &str, message: &str) -> bool {
    let mut dialog = app
        .dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancel);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.blocking_show()
}

pub fn confirm_exit(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    if state.closing.load(Ordering::SeqCst) || state.prompting.swap(true, Ordering::SeqCst) {
        return;
    }
    // Tray, menu-bar and OS quit requests can arrive while the only window is
    // hidden or minimized. Restore the stable owner before opening the modal so
    // the original exit question cannot remain behind an invisible window.
    crate::restore(app);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let prompt = handle.clone();
        let result = tauri::async_runtime::spawn_blocking(move || -> Result<bool> {
            let desktop = prompt.state::<Desktop>();
            let emby = prompt.state::<crate::emby::EmbyDesktop>();
            let maintenance = emby.maintenance_context()?;
            let maintenance_running = maintenance.is_some();
            // Use the service/legacy snapshot captured before the owned file
            // transaction took the session lock. The bounded wait belongs only
            // after the user explicitly confirms exit.
            let (service, legacy_blocked) = if let Some(context) = maintenance {
                (
                    product_core::card_service::ServiceStatus {
                        phase: context.phase,
                        last_started_at: None,
                        lease: None,
                        error: None,
                    },
                    context.legacy_blocked,
                )
            } else {
                (
                    emby.service_status()?,
                    crate::emby::legacy_blocked(&prompt)?,
                )
            };
            let job = desktop.store.manager_job()?.unwrap_or_default();
            let job_running = maintenance_running
                || job["running"].as_bool().unwrap_or(false)
                || desktop.store.tasks()?.iter().any(|task| {
                    matches!(
                        task.state,
                        TaskState::Requested
                            | TaskState::Running
                            | TaskState::Paused
                            | TaskState::Interrupted
                    )
                });
            let context = ExitContext {
                phase: service.phase,
                legacy_blocked,
                job_running,
                maintenance_running: maintenance_running
                    || (job["running"].as_bool().unwrap_or(false)
                        && job["blocks_exit"].as_bool().unwrap_or(false)),
            };
            let snapshot = crate::languages::snapshot(
                &prompt,
                &prompt.state::<crate::languages::Languages>(),
            )?;
            let message = exit_message(&snapshot, &context)?;
            let title = crate::languages::native_message(
                &snapshot,
                "退出 Tech Card Manager？",
                "Exit Tech Card Manager?",
            )?;
            Ok(exit_dialog(&prompt, &title, &message))
        })
        .await
        .map_err(|error| AppError::new("exit-confirmation-worker", error))
        .and_then(|value| value);
        handle
            .state::<Lifecycle>()
            .prompting
            .store(false, Ordering::SeqCst);
        match result {
            Ok(true) => request_exit(&handle),
            Ok(false) => crate::restore(&handle),
            Err(error) => {
                set_error(&handle, error);
                crate::restore(&handle);
            }
        }
    });
}
pub fn request_exit(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    if state.closing.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let _ = app.emit("lifecycle-closing", true);
    tauri::async_runtime::spawn_blocking(move || match crate::prepare_exit(&handle) {
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
            crate::restore(&handle);
            set_error(&handle, error.clone());
            show_shutdown_error(&handle, &error);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_checkbox_uses_native_registration_without_rewriting_saved_silent_choice() {
        let temp = tempfile::tempdir().unwrap();
        let store = product_core::store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let saved = Settings {
            revision: 1,
            launch_at_login: true,
            start_hidden: true,
            close_action: CloseAction::Background,
        };
        store
            .save_preference("lifecycle-settings", &serde_json::to_value(&saved).unwrap())
            .unwrap();
        let observed = settings_for_presentation(&store, &Ok(false)).unwrap();
        assert!(!observed.launch_at_login);
        assert!(observed.start_hidden);
        assert_eq!(observed.close_action, CloseAction::Quit);
        assert_eq!(store.lifecycle_settings().unwrap(), saved);
        assert!(
            settings_for_presentation(&store, &Ok(true))
                .unwrap()
                .launch_at_login
        );
    }
    #[test]
    fn missing_tray_keeps_a_system_window_restore_entry() {
        assert!(!background_hides(false));
        assert_eq!(background_hides(true), !cfg!(target_os = "linux"));
    }
}
