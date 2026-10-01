use product_core::{store::Store, *};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct Desktop {
    pub store: Arc<Store>,
    startup_source: Option<PathBuf>,
    startup_state: Mutex<StartupState>,
}
struct StartupState {
    error: Option<AppError>,
    announcement_pending: bool,
}
fn portable_upgrade_source(os: &str, identifier: &str, executable: &Path) -> Option<PathBuf> {
    if os != "windows" || identifier.to_ascii_lowercase().contains("validation") {
        return None;
    }
    executable.parent().map(Path::to_path_buf)
}
fn restore_startup_data(
    store: &Store,
    adjacent: &Path,
) -> Result<Option<product_core::migration::MigrationReceipt>> {
    store.import_portable_from_startup_sources(adjacent, || {
        #[cfg(windows)]
        {
            product_core::windows_startup::legacy_portable()
        }
        #[cfg(not(windows))]
        {
            Ok(None)
        }
    })
}
impl Desktop {
    pub fn start(app: &tauri::AppHandle) -> Result<Self> {
        let path = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::new("data-directory", e))?;
        std::fs::create_dir_all(&path).map_err(|e| AppError::new("data-directory", e))?;
        let store = Arc::new(Store::open(&path.join("workspace.sqlite"))?);
        store.retire_unowned_scans()?;
        let executable =
            std::env::current_exe().map_err(|e| AppError::new("application-path", e))?;
        let startup_source =
            portable_upgrade_source(std::env::consts::OS, &app.config().identifier, &executable);
        let startup_error = startup_source
            .as_ref()
            .and_then(|source| restore_startup_data(&store, source).err());
        Ok(Self {
            store,
            startup_source,
            startup_state: Mutex::new(StartupState {
                error: startup_error,
                announcement_pending: false,
            }),
        })
    }
    fn ensure_startup_migration(
        &self,
        announce: impl FnOnce(&Configuration) -> Result<()>,
    ) -> Result<()> {
        let mut state = self
            .startup_state
            .lock()
            .map_err(|e| AppError::new("migration-state", e))?;
        if state.error.is_some() {
            if let Some(source) = &self.startup_source {
                match restore_startup_data(&self.store, source) {
                    Ok(receipt) => {
                        state.error = None;
                        state.announcement_pending = receipt.is_some();
                    }
                    Err(error) => state.error = Some(error),
                }
            }
        }
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if state.announcement_pending {
            // Retry notification, not the already committed import, if the
            // event transport fails. All configuration readers share this gate.
            announce(&self.store.configuration()?)?;
            state.announcement_pending = false;
        }
        Ok(())
    }
}
#[tauri::command]
pub async fn configuration(app: tauri::AppHandle) -> Result<Configuration> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Desktop>();
        state.ensure_startup_migration(|config| {
            app.emit("configuration-changed", config)
                .map_err(|e| AppError::new("configuration-event", e))
        })?;
        state.store.configuration()
    })
    .await
    .map_err(|e| AppError::new("migration-worker", e))?
}
#[tauri::command]
pub fn operation_result(id: String, state: State<'_, Desktop>) -> Result<OperationResult> {
    state.store.operation_result(&id)
}
/// Folder selection is a draft input; only save_library_roots persists it.
#[tauri::command]
pub async fn choose_library_root(app: tauri::AppHandle) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot =
            crate::languages::snapshot(&app, &app.state::<crate::languages::Languages>())?;
        let title = crate::languages::native_message(
            &snapshot,
            "选择电影或电视剧媒体目录",
            "Select a Movie or TV Show media folder",
        )?;
        let Some(selected) = app.dialog().file().set_title(title).blocking_pick_folder() else {
            return Ok(None);
        };
        let selected = selected
            .into_path()
            .map_err(|e| AppError::new("invalid-path", e))?;
        let real = product_core::paths::checked(&selected)?;
        if !real.is_dir() {
            return Err(
                AppError::new("invalid-root", "Library root must be a directory")
                    .at(real.display()),
            );
        }
        let value = real
            .to_str()
            .ok_or_else(|| AppError::new("invalid-encoding", "Path must be Unicode"))?;
        Ok(Some(value.to_owned()))
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
}
#[tauri::command]
pub async fn save_library_roots(
    id: String,
    configuration: Configuration,
    app: tauri::AppHandle,
) -> Result<Configuration> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = app.state::<Desktop>().store.configure(&id, configuration)?;
        app.emit("configuration-changed", &saved)
            .map_err(|e| AppError::new("configuration-event", e))?;
        Ok(saved)
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
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
            .set_title(crate::languages::native_message(
                &crate::languages::snapshot(&app, &app.state::<crate::languages::Languages>())?,
                "选择电影或电视剧媒体目录",
                "Select a Movie or TV Show media folder",
            )?)
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
            id: product_core::hash(
                format!(
                    "{}:{path}",
                    if space == Space::Movie { "movie" } else { "tv" }
                )
                .as_bytes(),
            ),
            space,
            path,
        });
        state.store.configure(&operation_id, config).map(Some)
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
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
        let path = containing_directory(
            std::path::Path::new(&root.path),
            std::path::Path::new(&item.path),
        )?;
        open_folder(&path)
    })
    .await
    .map_err(|e| AppError::new("worker-failed", e))?
}
fn containing_directory(
    root: &std::path::Path,
    item: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let parent = item
        .parent()
        .ok_or_else(|| AppError::new("invalid-path", "Missing NFO parent directory"))?;
    product_core::paths::within(root, parent)
}

pub(crate) fn open_folder(path: &std::path::Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer.exe")
        .arg(path)
        .status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open").arg(path).status();
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
pub fn manager_catalog(state: State<'_, Desktop>) -> Result<Vec<product_core::ui::ManagerRow>> {
    Ok(product_core::ui::manager_catalog(state.store.all_items()?))
}

#[tauri::command]
pub fn catalog_summary(state: State<'_, Desktop>) -> Result<product_core::ui::CatalogSummary> {
    state.store.catalog_summary()
}
#[tauri::command]
pub fn catalog_members(
    space: Space,
    view: product_core::ui::LibraryView,
    state: State<'_, Desktop>,
) -> Result<Vec<String>> {
    state.store.catalog_members(space, view)
}

#[tauri::command]
pub async fn folder_settings(app: tauri::AppHandle) -> Result<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = app.state::<Desktop>();
        desktop.ensure_startup_migration(|config| app.emit("configuration-changed",config).map_err(|e| AppError::new("configuration-event",e)))?;
        let store = &desktop.store;
        let settings = store.folder_settings()?;
        let environment = store.preferences("emby-environment")?;
        let (discovered, discovery_error) = match environment.get("data").and_then(|v| v.as_str()) {
            Some(data) => match store.discovered_libraries(data) {
                Ok(rows) => (rows,None),
                Err(error) => (Vec::new(),Some(error)),
            },
            None => (Vec::new(),None),
        };
        let roots_configured = !settings.folders.is_empty()
            || store.preferences("media-folders")?.get("folders").is_some();
        let online: std::collections::BTreeMap<_, _> = settings
            .folders
            .iter()
            .map(|folder| {
                (
                    folder.id.clone(),
                    product_core::paths::checked(std::path::Path::new(&folder.path))
                        .is_ok_and(|p| p.is_dir()),
                )
            })
            .collect();
        Ok(serde_json::json!({"settings":settings,"online":online,"discovered":discovered,"discovery_error":discovery_error,"roots_configured":roots_configured}))
    })
    .await
    .map_err(|e| AppError::new("folders-worker", e))?
}
#[tauri::command]
pub async fn save_media_folders(
    id: String,
    settings: product_core::folders::FolderSettings,
    app: tauri::AppHandle,
) -> Result<product_core::folders::FolderReceipt> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = app
            .state::<Desktop>()
            .store
            .save_folder_settings(&id, settings)?;
        if let Err(e) = app.emit("configuration-changed", &saved.configuration) {
            eprintln!("configuration-event: {e}");
        }
        Ok(saved)
    })
    .await
    .map_err(|e| AppError::new("folders-worker", e))?
}

#[cfg(test)]
mod containing_directory_tests {
    use super::*;
    #[test]
    fn startup_failure_remains_actionable_and_configuration_retry_can_finish_import() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("portable");
        std::fs::create_dir_all(source.join("runtime/engine")).unwrap();
        std::fs::create_dir_all(source.join("data")).unwrap();
        std::fs::write(
            source.join("runtime/engine/windows-engine.ps1"),
            include_bytes!("../../../windows/engine/windows-engine.ps1"),
        )
        .unwrap();
        std::fs::write(source.join("data/settings.json"), b"{invalid").unwrap();
        let store = Arc::new(Store::open(&root.join("new.sqlite")).unwrap());
        let error = store.import_portable_on_start(&source).unwrap_err();
        let desktop = Desktop {
            store,
            startup_source: Some(source.clone()),
            startup_state: Mutex::new(StartupState {
                error: Some(error),
                announcement_pending: false,
            }),
        };
        assert_eq!(
            desktop
                .ensure_startup_migration(|_| panic!("Failed import must not be announced"))
                .unwrap_err()
                .code,
            "migration-settings-invalid"
        );
        assert_eq!(desktop.store.configuration().unwrap().revision, 0);
        std::fs::write(
            source.join("data/settings.json"),
            br#"{"language":"en-US","roots_configured":false}"#,
        )
        .unwrap();
        assert_eq!(
            desktop
                .ensure_startup_migration(|configuration| {
                    assert_eq!(configuration.locale, Locale::English);
                    Err(AppError::new(
                        "configuration-event",
                        "fixture delivery failure",
                    ))
                })
                .unwrap_err()
                .code,
            "configuration-event"
        );
        assert_eq!(
            desktop.store.configuration().unwrap().locale,
            Locale::English
        );
        std::fs::rename(&source, root.join("offline")).unwrap();
        let mut announcements = 0;
        desktop
            .ensure_startup_migration(|configuration| {
                announcements += 1;
                assert_eq!(configuration.revision, 1);
                Ok(())
            })
            .unwrap();
        desktop
            .ensure_startup_migration(|_| panic!("Successful notification must not repeat"))
            .unwrap();
        assert_eq!(announcements, 1);
        assert!(desktop.startup_state.lock().unwrap().error.is_none());
    }
    #[test]
    fn complete_emby_summary_unblocks_a_broken_portable_settings_retry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let portable = root.join("portable");
        std::fs::create_dir_all(portable.join("runtime/engine")).unwrap();
        std::fs::create_dir_all(portable.join("data")).unwrap();
        std::fs::write(
            portable.join("runtime/engine/windows-engine.ps1"),
            include_bytes!("../../../windows/engine/windows-engine.ps1"),
        )
        .unwrap();
        std::fs::write(portable.join("data/settings.json"), b"{invalid").unwrap();
        let store = Arc::new(Store::open(&root.join("new.sqlite")).unwrap());
        let error = store.import_portable_on_start(&portable).unwrap_err();

        let roaming = root.join("roaming");
        let web = roaming.join("Emby-Server/system/dashboard-ui");
        let state = roaming.join("Emby-Server/programdata/custom-tech-specs");
        let media = root.join("old media");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir(&media).unwrap();
        std::fs::write(
            state.join("technical-specs-worker.ps1"),
            include_bytes!("../../../windows/engine/windows-engine.ps1"),
        )
        .unwrap();
        std::fs::write(
            state.join("manager-index-summary.json"),
            serde_json::to_vec(&serde_json::json!({
                "generatedAt": "2026-09-12T12:00:00Z",
                "scanStats": {},
                "items": {},
                "libraries": [{"name": "Recovered", "path": media, "kind": "Movies"}]
            }))
            .unwrap(),
        )
        .unwrap();
        store
            .import_original_emby_diagnostics("windows", "org.tcm.product", &web, Some(&roaming))
            .unwrap()
            .unwrap();

        let desktop = Desktop {
            store,
            startup_source: Some(portable),
            startup_state: Mutex::new(StartupState {
                error: Some(error),
                announcement_pending: false,
            }),
        };
        desktop
            .ensure_startup_migration(|_| panic!("Summary recovery is read on retry"))
            .unwrap();
        assert!(desktop.startup_state.lock().unwrap().error.is_none());
        assert_eq!(desktop.store.configuration().unwrap().roots.len(), 1);
        assert_eq!(
            desktop.store.folder_settings().unwrap().folders[0].name,
            "Recovered"
        );
    }
    #[test]
    fn startup_source_is_adjacent_windows_portable_only_and_validation_is_isolated() {
        let executable = Path::new("/portable/Tech-Card-Manager.exe");
        assert_eq!(
            portable_upgrade_source("windows", "fixture.product", executable),
            Some(PathBuf::from("/portable"))
        );
        for (os, id) in [
            ("windows", "io.github.eric-hou1997.tcm.validation"),
            ("windows", "fixture.Validation.UI"),
            ("macos", "fixture.product"),
            ("linux", "fixture.product"),
        ] {
            assert!(portable_upgrade_source(os, id, executable).is_none());
        }
    }
    #[test]
    fn missing_nfo_can_reveal_only_its_existing_real_library_parent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let library = root.join("library");
        let outside = root.join("outside");
        std::fs::create_dir(&library).unwrap();
        std::fs::create_dir(&outside).unwrap();
        assert_eq!(
            containing_directory(&library, &library.join("missing.nfo")).unwrap(),
            library
        );
        assert!(containing_directory(&library, &outside.join("missing.nfo")).is_err());
        assert!(containing_directory(&library, &library.join("../outside/missing.nfo")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, library.join("link")).unwrap();
            assert!(containing_directory(&library, &library.join("link/missing.nfo")).is_err());
        }
    }
}
