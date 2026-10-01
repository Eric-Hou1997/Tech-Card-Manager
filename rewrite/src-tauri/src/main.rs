#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[allow(dead_code)] // retained validation source; not registered in the product IPC surface
mod credentials;
mod desktop;
mod diagnostics;
mod dialogs;
mod emby;
mod languages;
mod lifecycle;
mod manual_update;
mod privileged;
mod update;
use product_core::services::CredentialStore;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
const PRODUCT: &str = "TCM";

fn prepare_exit(app: &tauri::AppHandle) -> product_core::Result<()> {
    app.state::<emby::EmbyDesktop>().shutdown()?;
    app.state::<lifecycle::Lifecycle>()
        .allow_exit
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
#[allow(dead_code)]
fn runtime_probe(nonce: String) -> Result<Value, String> {
    if nonce.len() > 128 {
        return Err("invalid-nonce".into());
    }
    Ok(
        json!({"product": PRODUCT, "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "echo": nonce}),
    )
}
fn report_event(event: &str) -> Result<(), String> {
    if let Some(path) = std::env::var_os("REWRITE_PROBE_REPORT") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{}", json!({"product":PRODUCT,"event":event,"os":std::env::consts::OS,"arch":std::env::consts::ARCH})).map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
fn frontend_ready(app: tauri::AppHandle) -> Result<(), String> {
    report_event("frontend-mounted-ipc-roundtrip")?;
    if std::env::var_os("REWRITE_PROBE_AUTOCLOSE").is_some() {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            lifecycle::request_exit(&app);
        });
    }
    Ok(())
}
#[tauri::command]
#[allow(dead_code)]
async fn directory_probe(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app.dialog().file().set_title("只读检查目录 / Read-only directory check").blocking_pick_folder() else { return Ok(json!({"status":"cancelled"})); };
        let path=selected.into_path().map_err(|e|e.to_string())?;
        let mut items=std::fs::read_dir(&path).map_err(|e|format!("directory-read: {e}"))?;
        let count=items.by_ref().take(1000).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?.len();
        Ok(json!({"status":"directory-readable","entries_sampled":count,"limit":1000,"modified":false,"emby_write_permission":"unverified"}))
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
#[allow(dead_code)]
fn storage_probe(app: tauri::AppHandle) -> Result<Value, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let work = tempfile::tempdir_in(dir).map_err(|e| e.to_string())?;
    let mut candidate = tempfile::NamedTempFile::new_in(work.path()).map_err(|e| e.to_string())?;
    candidate
        .write_all(b"validation-only")
        .map_err(|e| e.to_string())?;
    candidate.as_file().sync_all().map_err(|e| e.to_string())?;
    let destination = work.path().join("probe.txt");
    candidate.persist(&destination).map_err(|e| e.to_string())?;
    let mut data = String::new();
    std::fs::File::open(destination)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut data)
        .map_err(|e| e.to_string())?;
    if data != "validation-only" {
        return Err("readback-mismatch".into());
    }
    work.close().map_err(|e| format!("cleanup-failed: {e}"))?;
    Ok(
        json!({"status":"write-sync-read-cleanup-passed","scope":"validation-app-data-only","nfo_transaction":"not-tested"}),
    )
}
#[tauri::command]
#[allow(dead_code)]
async fn credential_probe() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let service=format!("io.github.eric-hou1997.{}.validation",PRODUCT.to_lowercase());
        let account=format!("probe-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos());
        let entry=credentials::NativeCredentials::new(service);
        entry.put(&account,"non-secret-validation-sentinel").map_err(|e|format!("credential-write: {e}"))?;
        let read=entry.get(&account);
        let cleanup=entry.delete(&account);
        cleanup.map_err(|e|format!("credential-cleanup-failed: {e}"))?;
        if read.map_err(|e|format!("credential-read: {e}"))?.as_deref() != Some("non-secret-validation-sentinel") {return Err("credential-roundtrip-mismatch".into());}
        if !matches!(entry.get(&account),Ok(None)) {return Err("credential-delete-unverified".into());}
        Ok(json!({"status":"native-store-roundtrip-and-delete-passed","production_credentials_accessed":false}))
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
#[allow(dead_code)]
async fn network_probe() -> Result<Value, String> {
    let url = "https://tauri.app/";
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("network-request: {e}"))?;
    Ok(
        json!({"status":response.status().as_u16(),"http_received":true,"emby_integration_verified":false,"note":"HTTP connectivity does not prove Emby integration"}),
    )
}
#[tauri::command]
fn quit_probe(app: tauri::AppHandle) {
    lifecycle::request_exit(&app);
}
fn restore(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        for result in [w.show(), w.unminimize(), w.set_focus()] {
            if let Err(e) = result {
                eprintln!("window-restore: {e}");
            }
        }
    }
}
fn show_startup_error(app: &tauri::AppHandle, error: &str) {
    let localized = app
        .try_state::<languages::Languages>()
        .and_then(|state| languages::snapshot(app, &state).ok());
    let text = localized
        .as_ref()
        .and_then(|snapshot| {
            languages::native_message(
                snapshot,
                "无法打开 Tech Card Manager 可视化界面，程序不会在后台继续运行。",
                "Tech Card Manager could not open its window and will not continue running in the background.",
            )
            .ok()
        })
        .unwrap_or_else(|| {
            "无法打开 Tech Card Manager 可视化界面，程序不会在后台继续运行。\n\nTech Card Manager could not open its window and will not continue running in the background.".into()
        });
    let title = localized
        .as_ref()
        .and_then(|snapshot| {
            languages::native_message(
                snapshot,
                "Tech Card Manager 启动失败",
                "Tech Card Manager startup failed",
            )
            .ok()
        })
        .unwrap_or_else(|| "Tech Card Manager 启动失败 / Startup Failed".into());
    let detail = languages::backend_message(app, error);
    let _ = app
        .dialog()
        .message(format!("{text}\n\n{detail}"))
        .title(title)
        .kind(MessageDialogKind::Error)
        .blocking_show();
}
fn main() {
    if product_core::lifecycle::ignored_agent_launch(std::env::args_os()) {
        // Same deprecated entry as 4.1.0. Do not initialize Tauri, forward a
        // second launch, import data, or recreate an independent resident Agent.
        let _ = writeln!(
            std::io::stderr(),
            "ignored deprecated --agent launch; open the visual Manager instead"
        );
        return;
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            restore(app);
            if let Err(e) = report_event("second-launch-forwarded") {
                eprintln!("{e}");
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    lifecycle::close_requested(window.app_handle());
                }
                #[cfg(not(target_os = "linux"))]
                if matches!(event, tauri::WindowEvent::Resized(_))
                    && window.is_minimized().unwrap_or(false)
                    && window
                        .app_handle()
                        .state::<lifecycle::Lifecycle>()
                        .tray
                        .load(std::sync::atomic::Ordering::SeqCst)
                {
                    if let Err(error) = window.hide() {
                        eprintln!("window-background: {error}");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            lifecycle::lifecycle_status,
            lifecycle::lifecycle_apply,
            lifecycle::background_window,
            frontend_ready,
            quit_probe,
            update::update_identity,
            manual_update::check_card_update,
            languages::language_status,
            languages::choose_language,
            languages::restore_language_packs,
            manual_update::open_card_update,
            manual_update::open_product_link,
            desktop::configuration,
            desktop::folder_settings,
            desktop::save_media_folders,
            desktop::operation_result,
            desktop::add_library_root,
            desktop::choose_library_root,
            desktop::save_library_roots,
            emby::scan_library,
            emby::scan_media_folder,
            desktop::task_history,
            diagnostics::diagnose,
            diagnostics::manager_job,
            dialogs::confirm_product_action,
            diagnostics::export_diagnostics,
            desktop::task_result,
            desktop::catalog,
            desktop::ui_state,
            desktop::save_ui_state,
            desktop::browse,
            desktop::catalog_members,
            desktop::catalog_summary,
            desktop::manager_catalog,
            desktop::inspector,
            emby::emby_select,
            emby::emby_authorize,
            emby::emby_authorization_available,
            emby::emby_path_mappings,
            emby::emby_save_mappings,
            emby::emby_libraries,
            emby::emby_add_library,
            emby::emby_discover,
            emby::emby_environment,
            emby::emby_connect,
            emby::emby_check_server,
            emby::emby_data_directory,
            emby::emby_status,
            emby::emby_legacy_components,
            emby::emby_legacy_operation,
            emby::emby_migrate_legacy_system,
            emby::emby_plan,
            emby::emby_apply,
            emby::emby_repair,
            emby::emby_operation,
            emby::emby_start,
            emby::rebuild_index,
            emby::refresh_libraries,
            emby::emby_stop,
            emby::emby_service_status,
            emby::incremental_status,
            emby::incremental_settings,
            emby::save_incremental_settings,
            desktop::reveal_item
        ])
        .setup(|app| {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                app.manage(desktop::Desktop::start(app.handle())?);
                app.manage(lifecycle::Lifecycle::default());
                app.manage(languages::Languages::default());
                app.manage(diagnostics::Diagnostics::default());
                lifecycle::initialize(app.handle());
                app.manage(manual_update::ManualUpdates::default());
                app.manage(emby::EmbyDesktop::new(
                    app.path().app_data_dir()?.join("emby-backups"),
                ));
                emby::initialize(app.handle());
                let show = MenuItem::with_id(app, "show", "显示窗口 / Show", true, None::<&str>)?;
                let quit =
                    MenuItem::with_id(app, "quit", "退出 / Quit", true, Some("CmdOrCtrl+Q"))?;
                // macOS menu bars require top-level submenus. A flat tray menu
                // cannot also serve as the menu bar: its accelerators stay inactive.
                let application = Submenu::with_items(app, "TCM", true, &[&show, &quit])?;
                app.set_menu(Menu::with_items(app, &[&application])?)?;
                let tray_show =
                    MenuItem::with_id(app, "show", "显示窗口 / Show", true, None::<&str>)?;
                let tray_quit = MenuItem::with_id(app, "quit", "退出 / Quit", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&tray_show, &tray_quit])?;
                app.manage(languages::LanguageMenus(
                    vec![show.clone(), tray_show.clone()],
                    vec![quit.clone(), tray_quit.clone()],
                ));
                let language =
                    languages::snapshot(app.handle(), &app.state::<languages::Languages>())?;
                languages::synchronize_menus(app.handle(), &language)?;
                app.on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => restore(app),
                    "quit" => lifecycle::confirm_exit(app),
                    _ => {}
                });
                let mut tray = TrayIconBuilder::new()
                    .menu(&tray_menu)
                    .tooltip("Tech Card Manager")
                    .on_tray_icon_event(|tray, event| {
                        if matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            } | TrayIconEvent::DoubleClick {
                                button: MouseButton::Left,
                                ..
                            }
                        ) {
                            restore(tray.app_handle());
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                match tray.build(app) {
                    Ok(_) => app
                        .state::<lifecycle::Lifecycle>()
                        .tray
                        .store(true, std::sync::atomic::Ordering::SeqCst),
                    Err(error) => eprintln!("tray-unavailable: {error}"),
                }
                lifecycle::first_window(app.handle())?;
                report_event("native-setup-complete")?;
                Ok(())
            })();
            if let Err(error) = result {
                show_startup_error(app.handle(), &error.to_string());
                return Err(error);
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            eprintln!("application-setup: {error}");
            return;
        }
    };
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = &event {
            if !handle
                .state::<lifecycle::Lifecycle>()
                .allow_exit
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
                lifecycle::confirm_exit(handle);
            }
        }
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = &event {
            restore(handle);
        }
        if matches!(event, tauri::RunEvent::Exit) {
            if let Err(error) = handle.state::<emby::EmbyDesktop>().shutdown() {
                eprintln!("Emby shutdown failed: {error}");
            }
            if let Err(e) = report_event("process-exit") {
                eprintln!("{e}");
            }
        }
    });
}
