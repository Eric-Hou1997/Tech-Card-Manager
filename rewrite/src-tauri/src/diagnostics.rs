use crate::desktop::Desktop;
use product_core::diagnostics::DiagnosticFile;
use product_core::{AppError, Result};
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::Manager;
#[derive(Default)]
pub struct Diagnostics {
    snapshot: Mutex<Option<(String, Vec<DiagnosticFile>)>>,
}
impl Diagnostics {
    fn observe(&self, export: bool, id: String, files: Vec<DiagnosticFile>) -> Result<()> {
        if export {
            *self
                .snapshot
                .lock()
                .map_err(|error| AppError::new("diagnostics-state", error))? = Some((id, files));
        }
        Ok(())
    }
    fn files_for(&self, id: &str) -> Result<Vec<DiagnosticFile>> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|error| AppError::new("diagnostics-state", error))?;
        let (current, files) = snapshot.as_ref().ok_or_else(|| {
            AppError::new("diagnostics-missing", "Run diagnostics before exporting")
        })?;
        if current != id {
            return Err(AppError::new(
                "diagnostics-changed",
                "The reviewed diagnostic snapshot changed; run diagnostics again",
            ));
        }
        if !files.iter().any(|file| file.name == "settings.json") {
            return Err(AppError::new(
                "diagnostics-missing",
                "Prepare the diagnostic export before saving",
            ));
        }
        Ok(files.clone())
    }
}
fn current_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_secs().try_into().ok())
        .unwrap_or_default()
}
fn check<T: serde::Serialize>(result: Result<T>) -> Value {
    match result {
        Ok(value) => {
            json!({"state":"checked","observed_at":product_core::emby::timestamp(),"value":value})
        }
        Err(error) => {
            json!({"state":"failed","observed_at":product_core::emby::timestamp(),"error":error})
        }
    }
}
#[cfg(windows)]
fn legacy_observations(store: &product_core::store::Store) -> Value {
    let processes = product_core::legacy_process::observe();
    let startup = product_core::windows_startup::legacy_entries();
    let mut roots = std::collections::BTreeSet::new();
    let imported = store
        .startup_import_receipt()
        .map(|receipt| receipt.map(|receipt| receipt.source));
    if let Ok(Some(source)) = &imported {
        roots.insert(std::path::PathBuf::from(source));
    }
    if let Ok(processes) = &processes {
        for executable in processes
            .iter()
            .filter_map(|process| process.executable.as_ref())
            .filter(|exe| exe.baseline_assets_match)
        {
            if let Some(root) = std::path::Path::new(&executable.path).parent() {
                roots.insert(root.to_path_buf());
            }
        }
    }
    if let Ok(startup) = &startup {
        for executable in startup
            .iter()
            .filter(|entry| !entry.current_program)
            .filter_map(|entry| entry.executable.as_ref())
        {
            if let Some(root) = std::path::Path::new(executable).parent() {
                roots.insert(root.to_path_buf());
            }
        }
    }
    let agents:Vec<_>=roots.iter().map(|root|json!({"portable":root,"files":check(product_core::legacy_components::agent_files_at_known_source(root))})).collect();
    json!({"processes":check(processes),"startup":check(startup),"task":check(product_core::legacy_task::observe()),"agents":{"source":check(imported),"directories":agents}})
}
#[tauri::command]
pub async fn diagnose(export_file: Option<bool>, app: tauri::AppHandle) -> Result<Value> {
    if export_file != Some(false) {
        // Polling and export do not start or replace the original task log.
        return collect_diagnostics(export_file, app).await;
    }
    let store = app.state::<Desktop>().store.clone();
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!(
        "diagnose-{}-{}",
        product_core::emby::timestamp(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let start_store = store.clone();
    let start_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || start_store.begin_diagnostic_job(&start_id))
        .await
        .map_err(|e| AppError::new("diagnostics-worker", e))??;
    let result = collect_diagnostics(export_file, app).await;
    let output = result
        .as_ref()
        .map_err(Clone::clone)
        .and_then(diagnostic_output);
    let job = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
        store.finish_diagnostic_job(&id, &output)?;
        store.manager_job()
    })
    .await
    .map_err(|e| AppError::new("diagnostics-worker", e))??;
    let mut value = result?;
    value["report"]["runtime"]["job"] = check(Ok(job));
    Ok(value)
}
fn diagnostic_output(value: &Value) -> Result<product_core::diagnostics::IndexDiagnostic> {
    let report = &value["report"];
    for key in ["index_summary_error", "xml_errors_error"] {
        if !report["runtime"][key].is_null() {
            return Err(serde_json::from_value(report["runtime"][key].clone())?);
        }
    }
    let integration = &report["runtime"]["emby"]["value"];
    Ok(product_core::diagnostics::IndexDiagnostic {
        summary: serde_json::from_value(report["runtime"]["index_summary"].clone())?,
        frontend_ok: integration["installed"] == true
            && integration["details"]["script_matches"] == true
            && integration["details"]["script_version"] == env!("CARGO_PKG_VERSION"),
        xml_errors_path: serde_json::from_value(report["runtime"]["xml_errors_path"].clone())?,
    })
}

#[tauri::command]
pub async fn manager_job(app: tauri::AppHandle) -> Result<Option<Value>> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Desktop>().store.manager_job())
        .await
        .map_err(|error| AppError::new("diagnostics-worker", error))?
}

async fn collect_diagnostics(export_file: Option<bool>, app: tauri::AppHandle) -> Result<Value> {
    // The settings poll omits this argument. Scan processes only for an explicit
    // diagnostic/export action, never on the two-second status refresh.
    let inspect_legacy = export_file.is_some();
    let export_file = export_file.unwrap_or(false);
    let handle = app.clone();
    let (library, index_summary, summary_error, xml_path, xml_error, mut files, archived_logs, job) = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
        let state = handle.state::<Desktop>();
        let library = state.store.library_diagnostics()?;
        // The original archived summary can contain a complete media index.
        // Read/validate it for explicit diagnostics or export, never each poll.
        let (mut summary_source, summary_error) = if inspect_legacy {
            match state.store.diagnostic_index() { Ok(source)=>(source,None), Err(error)=>(None,Some(error)) }
        } else { (None,None) };
        let index_summary = summary_source.as_ref().map(|source| source.summary.clone());
        let (xml_bytes, xml_path, xml_error) = if inspect_legacy {
            let xml = match summary_source.as_mut() {
                Some(source) => std::mem::replace(&mut source.xml_errors, Ok(None))
                    .map(|source| source.map(|source| (source.bytes, source.modified_unix))),
                None => Ok(None),
            };
            match xml {
                Ok(Some((bytes, modified_unix))) => {
                    if export_file { (Some((bytes, modified_unix)), None, None) } else {
                        let path = handle.path().app_data_dir().map_err(|error| AppError::new("diagnostics-path", error)).and_then(|path| write_xml_detail(&path, &bytes));
                        match path { Ok(path) => (Some((bytes, modified_unix)),Some(path),None), Err(error) => (Some((bytes, modified_unix)),None,Some(error)) }
                    }
                }
                Ok(None) => (None,None,None),
                Err(error) => (None,None,Some(error)),
            }
        } else { (None,None,None) };
        let mut files = Vec::new();
        let mut archives = Vec::new();
        if export_file {
        let generated_unix = current_unix();
        let folders = state.store.folder_settings()?;
        let items = state.store.all_items()?;
        files = vec![
            DiagnosticFile { name:"settings.json".into(), bytes:serde_json::to_vec_pretty(&json!({"folders":folders,"locale":library.locale,"incremental":state.store.incremental_settings()?}))?, modified_unix:Some(generated_unix) },
            DiagnosticFile { name:"manager-catalog.json".into(), bytes:serde_json::to_vec_pretty(&items)?, modified_unix:Some(generated_unix) },
            DiagnosticFile { name:"task-history.json".into(), bytes:serde_json::to_vec_pretty(&state.store.tasks()?)?, modified_unix:Some(generated_unix) },
        ];
        if let Some((bytes, modified_unix)) = xml_bytes { files.push(DiagnosticFile{name:"manager-xml-errors.json".into(),bytes,modified_unix}); }
        if let Some(source) = summary_source { files.push(DiagnosticFile{name:"manager-index-summary.json".into(),bytes:source.summary_bytes,modified_unix:source.summary_modified_unix}); }
        if let Some(log) = state.store.manager_job_log()? { files.push(DiagnosticFile{name:"job.log".into(),bytes:log.into_bytes(),modified_unix:Some(generated_unix)}); }
        for (archive, bytes, modified_unix) in state.store.diagnostic_logs()? {
            if files.iter().any(|file|file.name == archive.path.rsplit('/').next().unwrap_or_default()) {continue;}
            files.push(DiagnosticFile { name:archive.path.rsplit('/').next().unwrap_or_default().into(), bytes, modified_unix });
            archives.push(archive);
        }
        }
        Ok((library, index_summary, summary_error, xml_path, xml_error, files, archives, check(state.store.manager_job())))
    })
    .await
    .map_err(|e| AppError::new("diagnostics-worker", e))??;
    // Current typed state and the original diagnostic log allowlist only.
    // Imported log provenance is recorded; old logs are not presented as new runs.
    let emby_result = crate::emby::emby_status(app.clone()).await;
    let index_current = emby_result
        .as_ref()
        .ok()
        .and_then(|status| status.as_ref())
        .is_some_and(|status| status.index_current);
    let emby = check(emby_result);
    let service = check(crate::emby::emby_service_status(app.clone()).await);
    let lifecycle = check(crate::lifecycle::lifecycle_status(app.clone()));
    let incremental = check(crate::emby::incremental_status(app.clone()).await);
    let mut report = json!({"schema":1,"product":"Tech Card Manager","version":app.package_info().version.to_string(),"created_at":product_core::emby::timestamp(),"platform":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"library":library,"runtime":{"emby":emby,"index_current":index_current,"service":service,"incremental":incremental,"lifecycle":lifecycle,"authorization_available":crate::emby::emby_authorization_available()}});
    report["archived_logs"] = serde_json::to_value(archived_logs)?;
    report["runtime"]["job"] = job;
    report["runtime"]["index_summary"] = serde_json::to_value(index_summary)?;
    report["runtime"]["index_summary_error"] = serde_json::to_value(&summary_error)?;
    report["runtime"]["xml_errors_path"] = serde_json::to_value(xml_path)?;
    report["runtime"]["xml_errors_error"] = serde_json::to_value(&xml_error)?;
    if inspect_legacy {
        #[cfg(windows)]
        {
            let observations = if app.config().identifier.ends_with(".validation") {
                let skipped = json!({"state":"unverified","reason":"validation-isolation"});
                json!({"processes":skipped,"startup":skipped,"task":skipped,"agents":skipped})
            } else {
                let store = app.state::<Desktop>().store.clone();
                tauri::async_runtime::spawn_blocking(move || legacy_observations(&store))
                    .await
                    .map_err(|e| AppError::new("legacy-components-worker", e))?
            };
            report["runtime"]["legacy_processes"] = observations["processes"].clone();
            report["runtime"]["legacy_startup"] = observations["startup"].clone();
            report["runtime"]["legacy_task"] = observations["task"].clone();
            report["runtime"]["legacy_agents"] = observations["agents"].clone();
        }
        #[cfg(not(windows))]
        {
            report["runtime"]["legacy_processes"] =
                json!({"state":"not-applicable","reason":"original-windows-processes"});
        }
    }
    let web = report["runtime"]["emby"]["value"]["target"]
        .as_str()
        .map(str::to_owned);
    let mut collection_errors = Vec::new();
    if let Some(error) = xml_error {
        collection_errors.push(json!({"file":"manager-xml-errors.json","error":error}));
    }
    if let Some(error) = summary_error {
        collection_errors.push(json!({"file":"manager-index-summary.json","error":error}));
    }
    if let Some(web) = web.filter(|_| export_file) {
        let collected = tauri::async_runtime::spawn_blocking(move || {
            let mut collected = Vec::new();
            for name in [
                "index.html",
                "technical-specs-data.json",
                "technical-specs-languages.json",
            ] {
                let result = read_diagnostic_file(std::path::Path::new(&web), name);
                collected.push((name, result));
            }
            collected
        })
        .await
        .map_err(|e| AppError::new("diagnostics-worker", e))?;
        for (name, result) in collected {
            match result {
                Ok(Some((bytes, modified_unix))) => files.push(DiagnosticFile {
                    name: name.into(),
                    bytes,
                    modified_unix: Some(modified_unix),
                }),
                Ok(None) => {}
                Err(error) => collection_errors.push(json!({"file":name,"error":error})),
            }
        }
    }
    report["collection_errors"] = serde_json::to_value(collection_errors)?;
    files.push(DiagnosticFile {
        name: "status.json".into(),
        bytes: serde_json::to_vec_pretty(&report)?,
        modified_unix: Some(current_unix()),
    });
    let identity: Vec<_> = files
        .iter()
        .map(|file| {
            (
                &file.name,
                product_core::hash(&file.bytes),
                file.modified_unix,
            )
        })
        .collect();
    let id = product_core::hash(&serde_json::to_vec(&identity)?);
    app.state::<Diagnostics>()
        .observe(export_file, id.clone(), files)?;
    Ok(json!({"id":id,"report":report}))
}
#[tauri::command]
pub async fn export_diagnostics(id: String, app: tauri::AppHandle) -> Result<Option<String>> {
    let bytes = app.state::<Diagnostics>().files_for(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let data = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::new("diagnostics-path", e))?;
        product_core::paths::checked(&data)?;
        let logs = data.join("logs");
        std::fs::create_dir_all(&logs)
            .map_err(|e| AppError::new("diagnostics-path", e).at(logs.display()))?;
        product_core::paths::checked(&logs)?;
        let base = product_core::diagnostics::archive_name();
        let mut destination = None;
        for suffix in 0..100 {
            let name = if suffix == 0 {
                base.clone()
            } else {
                format!("{}-{suffix}.zip", base.trim_end_matches(".zip"))
            };
            let path = logs.join(name);
            match product_core::diagnostics::write_archive(&path, &bytes) {
                Ok(()) => {
                    destination = Some(path);
                    break;
                }
                Err(error) if error.code == "diagnostics-exists" => {}
                Err(error) => return Err(error),
            }
        }
        let path = destination.ok_or_else(|| {
            AppError::new(
                "diagnostics-exists",
                "Too many diagnostic exports with this timestamp",
            )
        })?;
        Ok(Some(complete_export(&path, &logs, |folder| {
            crate::desktop::open_folder(folder)
        })))
    })
    .await
    .map_err(|e| AppError::new("diagnostics-worker", e))?
}

fn complete_export<F>(path: &std::path::Path, folder: &std::path::Path, open_folder: F) -> String
where
    F: FnOnce(&std::path::Path) -> Result<()>,
{
    let _ = open_folder(folder);
    path.to_string_lossy().into_owned()
}

fn write_xml_detail(root: &std::path::Path, bytes: &[u8]) -> Result<String> {
    use std::io::Write;
    product_core::paths::checked(root)?;
    let path = root.join("manager-xml-errors.json");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => {
            return Err(AppError::new(
                "diagnostics-path",
                "XML detail destination is not a regular file",
            )
            .at(path.display()))
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(AppError::new("diagnostics-path", error).at(path.display()))
        }
        _ => {}
    }
    let mut candidate = tempfile::NamedTempFile::new_in(root)
        .map_err(|error| AppError::new("diagnostics-write", error))?;
    candidate
        .write_all(bytes)
        .and_then(|_| candidate.as_file().sync_all())
        .map_err(|error| AppError::new("diagnostics-write", error).at(path.display()))?;
    candidate
        .persist(&path)
        .map_err(|error| AppError::new("diagnostics-write", error.error).at(path.display()))?;
    if std::fs::read(&path)
        .map_err(|error| AppError::new("diagnostics-read", error).at(path.display()))?
        != bytes
    {
        return Err(AppError::new(
            "diagnostics-changed",
            "XML detail file changed after writing",
        )
        .at(path.display()));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::new("diagnostics-path", "XML detail path must be Unicode"))
}

fn read_diagnostic_file(root: &std::path::Path, name: &str) -> Result<Option<(Vec<u8>, i64)>> {
    use std::io::Read;
    if !matches!(
        name,
        "index.html" | "technical-specs-data.json" | "technical-specs-languages.json"
    ) {
        return Err(AppError::new(
            "diagnostics-source",
            "Unsupported diagnostic source",
        ));
    }
    product_core::paths::checked(root)?;
    let path = root.join(name);
    let before = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::new("diagnostics-read", error).at(path.display())),
        Ok(metadata) if !metadata.is_file() => {
            return Err(
                AppError::new("diagnostics-read", "Expected a regular diagnostic file")
                    .at(path.display()),
            )
        }
        Ok(metadata) => metadata,
    };
    let path = product_core::paths::within(root, &path)?;
    let file = std::fs::File::open(&path)
        .map_err(|e| AppError::new("diagnostics-read", e).at(path.display()))?;
    let mut bytes = Vec::new();
    file.take(12 << 20)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::new("diagnostics-read", e).at(path.display()))?;
    let after = std::fs::symlink_metadata(&path)
        .map_err(|error| AppError::new("diagnostics-read", error).at(path.display()))?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err(AppError::new(
            "diagnostics-changed",
            "Diagnostic source changed while reading",
        )
        .at(path.display()));
    }
    let modified_unix = before
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_secs().try_into().ok())
        .ok_or_else(|| {
            AppError::new("diagnostics-time", "Diagnostic source time is unavailable")
                .at(path.display())
        })?;
    Ok(Some((bytes, modified_unix)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polling_cannot_replace_the_prepared_export_and_new_exports_reject_old_ids() {
        let state = Diagnostics::default();
        let files = || {
            vec![DiagnosticFile {
                name: "settings.json".into(),
                bytes: vec![1, 2, 3],
                modified_unix: None,
            }]
        };
        state.observe(false, "poll-before".into(), files()).unwrap();
        assert!(state.files_for("poll-before").is_err());
        state.observe(true, "reviewed".into(), files()).unwrap();
        state.observe(false, "poll-after".into(), vec![]).unwrap();
        assert_eq!(state.files_for("reviewed").unwrap()[0].bytes, vec![1, 2, 3]);
        state.observe(true, "new-export".into(), files()).unwrap();
        assert_eq!(
            state.files_for("reviewed").unwrap_err().code,
            "diagnostics-changed"
        );
    }
    #[test]
    fn xml_detail_file_is_real_atomic_and_unsafe_destinations_fail_without_a_false_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = write_xml_detail(&root, br#"{"count":1,"errors":[]}"#).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), br#"{"count":1,"errors":[]}"#);
        write_xml_detail(&root, br#"{"count":0,"errors":[]}"#).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), br#"{"count":0,"errors":[]}"#);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(write_xml_detail(&root, b"replace-directory").is_err());
        std::fs::remove_dir(&path).unwrap();
        #[cfg(unix)]
        {
            let original = root.join("keep.json");
            std::fs::write(&original, b"keep").unwrap();
            std::os::unix::fs::symlink(&original, &path).unwrap();
            assert!(write_xml_detail(&root, b"replace-link").is_err());
            assert_eq!(std::fs::read(&original).unwrap(), b"keep");
        }
        let error = AppError::new("diagnostics-write", "file could not be saved");
        assert_eq!(
            diagnostic_output(&json!({"report":{"runtime":{"xml_errors_error":error}}}))
                .err()
                .unwrap(),
            error
        );
    }
    #[test]
    fn failed_summary_read_is_a_failed_diagnostic_not_an_absent_index() {
        let error = AppError::new("migration-archive-corrupt", "changed original summary")
            .at("data/manager-index-summary.json");
        let report =
            json!({"report":{"runtime":{"index_summary":null,"index_summary_error":error}}});
        let observed = diagnostic_output(&report).err().unwrap();
        assert_eq!(observed, error);
        let absent =
            json!({"report":{"runtime":{"index_summary":null,"index_summary_error":null}}});
        assert!(diagnostic_output(&absent).unwrap().summary.is_none());
    }
    #[test]
    fn diagnostic_source_reads_are_bounded_and_missing_files_are_optional() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(read_diagnostic_file(&root, "index.html").unwrap(), None);
        std::fs::write(root.join("index.html"), vec![42; (12 << 20) + 7]).unwrap();
        assert_eq!(
            read_diagnostic_file(&root, "index.html")
                .unwrap()
                .unwrap()
                .0
                .len(),
            12 << 20
        );
        assert!(read_diagnostic_file(&root, "../index.html").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.join("index.html"),
                root.join("technical-specs-data.json"),
            )
            .unwrap();
            assert!(read_diagnostic_file(&root, "technical-specs-data.json").is_err());
        }
    }
    #[test]
    fn a_saved_export_stays_successful_when_its_folder_cannot_be_opened() {
        let path = std::path::Path::new("diagnostics/IMDb-Tech-Diagnostics.zip");
        let returned = complete_export(path, path.parent().unwrap(), |_| {
            Err(AppError::new("reveal-failed", "file manager unavailable"))
        });
        assert_eq!(returned, path.to_string_lossy());
    }
}
