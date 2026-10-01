use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};
use tcm_core::{
    folders::*,
    incremental::{IncrementalSettings, IncrementalWorker},
    store::Store,
    *,
};

fn wait(mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < until, "scan timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn setup() -> (tempfile::TempDir, Arc<Store>, IncrementalWorker) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let store = Arc::new(Store::open(&root.join("state.sqlite")).unwrap());
    let mut folders = vec![];
    for (name, kind) in [
        ("movies", FolderKind::Movies),
        ("tv", FolderKind::Tv),
        ("auto", FolderKind::Auto),
        ("mixed", FolderKind::Mixed),
    ] {
        let path = root.join(name);
        fs::create_dir(&path).unwrap();
        for (file, element) in [("movie.nfo", "movie"), ("tvshow.nfo", "tvshow")] {
            fs::write(
                path.join(file),
                format!("\u{feff}<{element}><title>Before {name}</title></{element}>\r\n"),
            )
            .unwrap();
        }
        folders.push(MediaFolder {
            id: name.into(),
            path: path.display().to_string(),
            name: name.into(),
            kind,
            source: FolderSource::Manual,
            enabled: true,
        });
    }
    store
        .save_folder_settings(
            "settings",
            FolderSettings {
                revision: 0,
                folders,
            },
        )
        .unwrap();
    store
        .save_incremental_settings(
            "interval",
            IncrementalSettings {
                revision: 0,
                interval_seconds: 3600,
            },
        )
        .unwrap();
    let worker = IncrementalWorker::start(store.clone(), "manager", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    (temp, store, worker)
}
fn change_sources(
    temp: &tempfile::TempDir,
) -> Vec<(std::path::PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut before = vec![];
    for name in ["movies", "tv", "auto", "mixed"] {
        for (file, element) in [("movie.nfo", "movie"), ("tvshow.nfo", "tvshow")] {
            let path = temp.path().join(name).join(file);
            let bytes =
                format!("\u{feff}<{element}><title>Changed title {name}</title></{element}>\r\n")
                    .into_bytes();
            fs::write(&path, &bytes).unwrap();
            before.push((
                path.clone(),
                bytes,
                fs::metadata(path).unwrap().modified().unwrap(),
            ));
        }
    }
    before
}
#[test]
fn space_refresh_reads_only_explicitly_classified_saved_roots() {
    let (temp, store, mut worker) = setup();
    let before = change_sources(&temp);
    let tasks = worker
        .request_manager_scan("movie-only", 1, ManagerScanScope::Space(Space::Movie))
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].roots.len(), 1);
    assert_eq!(
        std::path::Path::new(&tasks[0].roots[0].path),
        temp.path().canonicalize().unwrap().join("movies")
    );
    assert!(tasks[0].id.starts_with("scan-space-"));
    assert!(!tasks[0].force_parse);
    wait(|| store.task(&tasks[0].id).unwrap().state == TaskState::Completed);
    let items = store.all_items().unwrap();
    assert_eq!(
        items
            .iter()
            .filter(|i| i.title.starts_with("Changed"))
            .count(),
        1
    );
    assert!(items
        .iter()
        .filter(|i| i.title.starts_with("Changed"))
        .all(|i| i.space == Space::Movie
            && std::path::Path::new(&i.path).parent()
                == Some(temp.path().canonicalize().unwrap().join("movies").as_path())));
    worker.stop().unwrap();
    for (path, bytes, mtime) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), mtime);
    }
    assert_eq!(
        worker
            .request_manager_scan("stopped", 1, ManagerScanScope::Space(Space::Movie))
            .unwrap_err()
            .code,
        "service-stopped"
    );
}
#[test]
fn a_mixed_folder_is_one_atomic_service_owned_request_and_stop_cancels_queued_work() {
    let (temp, store, mut worker) = setup();
    let before = change_sources(&temp);
    let count = store.tasks().unwrap().len();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_second_scope BEFORE INSERT ON tasks WHEN NEW.id LIKE 'scan-root-%' AND json_extract(NEW.body,'$.space')='tv' BEGIN SELECT RAISE(ABORT,'second task failed'); END;").unwrap();
    assert!(worker
        .request_manager_scan("folder", 1, ManagerScanScope::Folder("mixed".into()))
        .is_err());
    assert_eq!(store.tasks().unwrap().len(), count);
    assert!(store.operation_result("folder").is_err());
    db.execute_batch("DROP TRIGGER fail_second_scope").unwrap();
    let tasks = worker
        .request_manager_scan("folder", 1, ManagerScanScope::Folder("mixed".into()))
        .unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.id.starts_with("scan-root-")
        && t.service_session.as_deref() == Some("manager")
        && t.roots.len() == 1
        && std::path::Path::new(&t.roots[0].path)
            == temp.path().canonicalize().unwrap().join("mixed")));
    match store.run_next(|| false, |_| {}) {
        Ok(None) => {}
        Err(error) if error.code == "worker-busy" => {}
        other => panic!("independent worker consumed service work: {other:?}"),
    }
    worker.stop().unwrap();
    assert!(tasks
        .iter()
        .all(|t| store.task(&t.id).unwrap().state.terminal()));
    assert_eq!(store.tasks().unwrap().len(), count + 2);
    for (path, bytes, mtime) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), mtime);
    }
}
#[test]
fn disabled_unknown_and_unclassified_scopes_never_fall_back_to_all_libraries() {
    let (_temp, store, mut worker) = setup();
    let mut settings = store.folder_settings().unwrap();
    for folder in &mut settings.folders {
        if matches!(folder.kind, FolderKind::Movies | FolderKind::Tv) {
            folder.enabled = false;
        }
    }
    let saved = store
        .save_folder_settings("disable-classified", settings)
        .unwrap();
    let count = store.tasks().unwrap().len();
    for scope in [
        ManagerScanScope::Folder("movies".into()),
        ManagerScanScope::Folder("unknown".into()),
    ] {
        assert_eq!(
            worker
                .request_manager_scan("invalid", saved.configuration.revision, scope)
                .unwrap_err()
                .code,
            "invalid-scope"
        );
    }
    let error = worker
        .request_manager_scan(
            "mixed-space",
            saved.configuration.revision,
            ManagerScanScope::Space(Space::Movie),
        )
        .unwrap_err();
    assert!(error.message.contains("已避免误扫另一类媒体"));
    assert_eq!(
        worker
            .request_manager_scan("stale", 1, ManagerScanScope::Folder("auto".into()))
            .unwrap_err()
            .code,
        "configuration-conflict"
    );
    assert_eq!(store.tasks().unwrap().len(), count);
    worker.stop().unwrap();
}
#[test]
fn completed_receipt_replays_without_rescanning_and_new_schema_preserves_v5_data() {
    let (temp, store, mut worker) = setup();
    let _before = change_sources(&temp);
    let tasks = worker
        .request_manager_scan("replay", 1, ManagerScanScope::Folder("auto".into()))
        .unwrap();
    wait(|| {
        tasks
            .iter()
            .all(|t| store.task(&t.id).unwrap().state == TaskState::Completed)
            && worker.status().unwrap().phase == "waiting"
    });
    let count = store.tasks().unwrap().len();
    let changed: Vec<_> = store
        .all_items()
        .unwrap()
        .into_iter()
        .filter(|i| i.title.starts_with("Changed"))
        .collect();
    assert_eq!(changed.len(), 2);
    assert!(changed
        .iter()
        .all(|i| std::path::Path::new(&i.path).parent()
            == Some(temp.path().canonicalize().unwrap().join("auto").as_path())));
    let replay = worker
        .request_manager_scan("replay", 1, ManagerScanScope::Folder("auto".into()))
        .unwrap();
    assert_eq!(replay[0].id, tasks[0].id);
    assert_eq!(store.tasks().unwrap().len(), count);
    assert_eq!(
        worker
            .request_manager_scan("replay", 1, ManagerScanScope::Space(Space::Movie))
            .unwrap_err()
            .code,
        "operation-conflict"
    );
    worker.stop().unwrap();
    let config = store.configuration().unwrap();
    drop(worker);
    drop(store);
    let path = temp.path().join("state.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("DELETE FROM operations WHERE id='replay'", [])
        .unwrap();
    db.pragma_update(None, "user_version", 5).unwrap();
    drop(db);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.configuration().unwrap(), config);
    assert_eq!(store.tasks().unwrap().len(), count);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
}

#[test]
fn retiring_old_independent_scans_preserves_history_and_never_resumes_them() {
    let (_temp, store, mut worker) = setup();
    worker.stop().unwrap();
    let root = store
        .configuration()
        .unwrap()
        .roots
        .into_iter()
        .find(|r| r.space == Space::Movie)
        .unwrap();
    let task = store
        .submit(ScanRequest {
            operation_id: "old-feasibility-task".into(),
            space: Space::Movie,
            root_ids: vec![root.id],
        })
        .unwrap();
    let completed: Vec<_> = store
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.state == TaskState::Completed)
        .collect();
    store.retire_unowned_scans().unwrap();
    let cancelled = store.task(&task.id).unwrap();
    assert_eq!(cancelled.state, TaskState::Cancelled);
    assert_eq!(cancelled.failure.as_ref().unwrap().code, "service-stopped");
    store.retire_unowned_scans().unwrap();
    assert_eq!(store.task(&task.id).unwrap(), cancelled);
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    for task in completed {
        assert_eq!(store.task(&task.id).unwrap(), task);
    }
}
