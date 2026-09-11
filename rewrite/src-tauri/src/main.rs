#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod credentials;
mod desktop;
mod emby;
mod lifecycle;
mod migration;
mod update;
use product_core::services::CredentialStore;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_dialog::DialogExt;
const PRODUCT: &str = "TCM";

#[tauri::command]
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
            app.exit(0);
        });
    }
    Ok(())
}
#[tauri::command]
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
    app.exit(0);
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
fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            restore(app);
            if let Err(e) = report_event("second-launch-forwarded") {
                eprintln!("{e}");
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    lifecycle::close_requested(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            runtime_probe,
            lifecycle::lifecycle_status,
            lifecycle::lifecycle_apply,
            lifecycle::background_window,
            frontend_ready,
            directory_probe,
            storage_probe,
            credential_probe,
            network_probe,
            quit_probe,
            migration::migration_plan,
            migration::migration_apply,
            migration::migration_result,
            update::update_identity,
            update::update_status,
            update::update_check,
            update::update_install,
            update::update_cancel,
            desktop::configuration,
            desktop::operation_result,
            desktop::add_library_root,
            desktop::scan_library,
            desktop::task_control,
            desktop::task_history,
            desktop::task_result,
            desktop::catalog,
            desktop::ui_state,
            desktop::save_ui_state,
            desktop::browse,
            desktop::tv_catalog,
            desktop::tv_members,
            desktop::inspector,
            emby::emby_select,
            emby::emby_discover,
            emby::emby_environment,
            emby::emby_connect,
            emby::emby_check_server,
            emby::emby_data_directory,
            emby::emby_status,
            emby::emby_plan,
            emby::emby_apply,
            emby::emby_start,
            emby::emby_stop,
            emby::emby_service_status,
            desktop::reveal_item
        ])
        .setup(|app| {
            app.manage(desktop::Desktop::start(app.handle())?);
            app.manage(lifecycle::Lifecycle::default());
            lifecycle::initialize(app.handle());
            app.manage(update::Updates::default());
            app.manage(emby::EmbyDesktop::new(
                app.path().app_data_dir()?.join("emby-backups"),
            ));
            app.state::<emby::EmbyDesktop>()
                .restore(&app.state::<desktop::Desktop>().store)?;
            let show = MenuItem::with_id(app, "show", "显示窗口 / Show", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 / Quit", true, Some("CmdOrCtrl+Q"))?;
            // macOS menu bars require top-level submenus. A flat tray menu
            // cannot also serve as the menu bar: its accelerators stay inactive.
            let application = Submenu::with_items(app, "TCM", true, &[&show, &quit])?;
            app.set_menu(Menu::with_items(app, &[&application])?)?;
            let tray_show = MenuItem::with_id(app, "show", "显示窗口 / Show", true, None::<&str>)?;
            let tray_quit = MenuItem::with_id(app, "quit", "退出 / Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&tray_show, &tray_quit])?;
            app.on_menu_event(|app, event| match event.id().as_ref() {
                "show" => restore(app),
                "quit" => app.exit(0),
                _ => {}
            });
            let mut tray = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("TCM 技术验证");
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
        })
        .build(tauri::generate_context!())
        .expect("validation application setup failed");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = &event {
            if !handle
                .state::<lifecycle::Lifecycle>()
                .allow_exit
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
                lifecycle::request_exit(handle);
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
            handle.state::<desktop::Desktop>().shutdown();
            if let Err(e) = report_event("process-exit") {
                eprintln!("{e}");
            }
        }
    });
}

fn prepare_update_exit(app: &tauri::AppHandle) -> product_core::Result<()> {
    app.state::<emby::EmbyDesktop>().shutdown()?;
    app.state::<desktop::Desktop>().shutdown();
    app.state::<lifecycle::Lifecycle>()
        .allow_exit
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}
