use product_core::{store::Store, *};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct Desktop {
    pub store: Arc<Store>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Desktop {
    pub fn start(app: &tauri::AppHandle) -> Result<Self> {
        let path = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::new("data-directory", e))?;
        std::fs::create_dir_all(&path).map_err(|e| AppError::new("data-directory", e))?;
        let store = Arc::new(Store::open(&path.join("workspace.sqlite"))?);
        let stop = Arc::new(AtomicBool::new(false));
        let desktop = Self {
            store,
            stop,
            worker: Mutex::new(None),
        };
        desktop.resume(app)?;
        Ok(desktop)
    }
    pub fn resume(&self, app: &tauri::AppHandle) -> Result<()> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|e| AppError::new("worker-state", e))?;
        if worker.is_some() {
            return Ok(());
        }
        self.stop.store(false, Ordering::SeqCst);
        let worker_store = self.store.clone();
        let worker_stop = self.stop.clone();
        let handle = app.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("library-worker".into())
                .spawn(move || {
                    while !worker_stop.load(Ordering::SeqCst) {
                        match worker_store.run_next(
                            || worker_stop.load(Ordering::SeqCst),
                            |task| {
                                if let Err(e) = handle.emit("task-changed", task) {
                                    eprintln!("task-event: {e}");
                                }
                            },
                        ) {
                            Ok(Some(_)) => {}
                            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                            Err(error) => {
                                let _ = handle.emit("worker-failed", &error);
                                eprintln!("library-worker: {error}");
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| AppError::new("worker-start", e))?,
        );
        Ok(())
    }
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                if worker.join().is_err() {
                    eprintln!("library-worker panicked during shutdown");
                }
            }
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.shutdown();
    }
}
#[tauri::command]
pub fn configuration(state: State<'_, Desktop>) -> Result<Configuration> {
    state.store.configuration()
}
#[tauri::command]
pub fn operation_result(id: String, state: State<'_, Desktop>) -> Result<OperationResult> {
    state.store.operation_result(&id)
}
#[tauri::command]
pub async fn add_library_root(
    app: tauri::AppHandle,
    space: Space,
    operation_id: String,
) -> Result<Option<Configuration>> {
    tauri::async_runtime::spawn_blocking(move || {
        match app.state::<Desktop>().store.operation_result(&operation_id) {
            Ok(OperationResult::Configuration(value)) => return Ok(Some(value)),
            Ok(_) => {
                return Err(AppError::new(
                    "operation-conflict",
                    "Operation ID belongs to another action",
                ))
            }
            Err(e) if e.code == "operation-not-found" => {}
            Err(e) => return Err(e),
        }
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("选择媒体根目录 / Select media root")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|e| AppError::new("invalid-path", e))?;
        let real = product_core::paths::checked(&path)?;
        let path = real
            .to_str()
            .ok_or_else(|| AppError::new("invalid-encoding", "Path must be Unicode"))?
            .to_owned();
        let state = app.state::<Desktop>();
        let mut config = state.store.configuration()?;
        config.roots.push(LibraryRoot {
            id: product_core::hash(path.as_bytes()),
            space,
            path,
        });
        state.store.configure(&operation_id, config).map(Some)
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
}
#[tauri::command]
pub fn scan_library(request: ScanRequest, state: State<'_, Desktop>) -> Result<Task> {
    state.store.submit(request)
}
#[tauri::command]
pub fn task_control(request: TaskControl, state: State<'_, Desktop>) -> Result<Task> {
    state.store.control(request)
}
#[tauri::command]
pub fn task_history(state: State<'_, Desktop>) -> Result<Vec<Task>> {
    state.store.tasks()
}
#[tauri::command]
pub fn task_result(id: String, state: State<'_, Desktop>) -> Result<Task> {
    state.store.task(&id)
}
#[tauri::command]
pub fn catalog(query: CatalogQuery, state: State<'_, Desktop>) -> Result<CatalogPage> {
    state.store.query(query)
}
#[tauri::command]
pub fn inspector(id: String, state: State<'_, Desktop>) -> Result<MediaItem> {
    state.store.item(&id)
}
#[tauri::command]
pub async fn reveal_item(id: String, app: tauri::AppHandle) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Desktop>();
        let item = state.store.item(&id)?;
        let config = state.store.configuration()?;
        let root = config
            .roots
            .iter()
            .find(|r| r.id == item.root_id)
            .ok_or_else(|| AppError::new("invalid-root", "Root no longer configured"))?;
        let path = product_core::paths::within(
            std::path::Path::new(&root.path),
            std::path::Path::new(&item.path),
        )?;
        reveal(&path)
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
}
fn reveal(path: &std::path::Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .status();
    if !status
        .map_err(|e| AppError::new("reveal-failed", e).at(path.display()))?
        .success()
    {
        return Err(
            AppError::new("reveal-failed", "System file manager did not open").at(path.display()),
        );
    }
    Ok(())
}

#[tauri::command]
pub fn ui_state(state: State<'_, Desktop>) -> Result<product_core::ui::UiState> {
    state.store.ui_state()
}
#[tauri::command]
pub fn save_ui_state(
    id: String,
    value: product_core::ui::UiState,
    state: State<'_, Desktop>,
) -> Result<product_core::ui::UiReceipt> {
    state.store.save_ui_state(&id, value)
}
#[tauri::command]
pub fn browse(
    space: Space,
    view: product_core::ui::LibraryView,
    state: State<'_, Desktop>,
) -> Result<CatalogPage> {
    state.store.browse(space, view)
}

#[tauri::command]
pub fn tv_catalog(
    view: product_core::ui::LibraryView,
    state: State<'_, Desktop>,
) -> Result<product_core::tv::TvPage> {
    Ok(product_core::tv::page(&state.store.all_items()?, &view))
}
#[tauri::command]
pub fn tv_members(id: String, state: State<'_, Desktop>) -> Result<Vec<String>> {
    product_core::tv::members(&state.store.all_items()?, &id)
}
