use std::{fs, path::Path};
use tcm_core::{store::Store, *};
fn fixture() -> (&'static str, &'static str) {
    ("电影.nfo","\u{feff}<movie>\r\n<title>电影 &amp; 测试</title><year>1967</year><uniqueid type=\"imdb\">tt0064757</uniqueid><tag>External</tag><tag>1.43 : 1 (scene)</tag><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item><item>ARRI</item></section><generatedtags owner=\"IMDb Tech Manager\" engine=\"local\"><tag>1.43:1</tag></generatedtags></technicalspecs></movie>")
}
fn setup() -> (tempfile::TempDir, Store, Configuration) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    fs::create_dir(dir.join("movies")).unwrap();
    fs::create_dir(dir.join("tv")).unwrap();
    let store = Store::open(&dir.join("state.sqlite")).unwrap();
    let config = store
        .configure(
            "setup",
            Configuration {
                revision: 0,
                locale: Locale::English,
                roots: vec![
                    LibraryRoot {
                        id: "movie".into(),
                        space: Space::Movie,
                        path: dir.join("movies").to_str().unwrap().into(),
                    },
                    LibraryRoot {
                        id: "tv".into(),
                        space: Space::Tv,
                        path: dir.join("tv").to_str().unwrap().into(),
                    },
                ],
            },
        )
        .unwrap();
    (temp, store, config)
}
fn request(id: &str) -> ScanRequest {
    ScanRequest {
        operation_id: id.into(),
        space: Space::Movie,
        root_ids: vec!["movie".into()],
    }
}
fn query(space: Space) -> CatalogQuery {
    CatalogQuery {
        space,
        search: String::new(),
        only_errors: false,
        offset: 0,
        limit: 100,
    }
}
#[test]
fn readonly_nfo_identity_scope_and_ownership() {
    let (_temp, store, config) = setup();
    let root = Path::new(&config.roots[0].path);
    let (name, raw) = fixture();
    let path = root.join(name);
    fs::write(&path, raw).unwrap();
    fs::write(root.join("duplicate.nfo"), raw).unwrap();
    fs::write(Path::new(&config.roots[1].path).join("tv.nfo"), raw).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let task = store.submit(request("scan1")).unwrap();
    assert_eq!(task.locale, Locale::English);
    store.run_next(|| false, |_| {}).unwrap();
    let page = store.query(query(Space::Movie)).unwrap();
    assert_eq!(page.total, 2);
    assert_ne!(page.items[0].id, page.items[1].id);
    assert_eq!(page.items[0].imdb, "tt0064757");
    assert_eq!(page.items[0].tags[0].ownership, Ownership::External);
    assert_eq!(page.items[0].tags[1].ownership, Ownership::Generated);
    assert_eq!(page.items[0].specs["Camera"], vec!["ARRI"]);
    assert_eq!(page.items[0].title, "电影 & 测试");
    assert_eq!(store.query(query(Space::Tv)).unwrap().total, 0);
    assert_eq!(fs::read(&path).unwrap(), raw.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}
#[test]
fn operation_replay_and_changed_input_conflict() {
    let (_temp, store, config) = setup();
    let r = request("same");
    let task = store.submit(r.clone()).unwrap();
    assert_eq!(store.submit(r).unwrap(), task);
    let mut changed = request("same");
    changed.space = Space::Tv;
    changed.root_ids = vec!["tv".into()];
    assert_eq!(
        store.submit(changed).unwrap_err().code,
        "operation-conflict"
    );
    assert_eq!(store.tasks().unwrap().len(), 1);
    let mut settings = config;
    settings.locale = Locale::Traditional;
    let saved = store.configure("locale", settings.clone()).unwrap();
    assert_eq!(store.configure("locale", settings).unwrap(), saved);
    assert_eq!(store.task("same").unwrap().locale, Locale::English);
}
#[test]
fn cancel_during_progress_is_not_overwritten_by_worker() {
    let (_temp, store, config) = setup();
    for i in 0..4 {
        fs::write(
            Path::new(&config.roots[0].path).join(format!("{i}.nfo")),
            fixture().1,
        )
        .unwrap();
    }
    store.submit(request("cancel-me")).unwrap();
    store
        .run_next(
            || false,
            |task| {
                if task.processed == 1 {
                    store
                        .control(TaskControl {
                            operation_id: "cancel1".into(),
                            task_id: task.id.clone(),
                            state: TaskState::Cancelled,
                        })
                        .unwrap();
                }
            },
        )
        .unwrap();
    let task = store.task("cancel-me").unwrap();
    assert_eq!(task.state, TaskState::Cancelled);
    assert_eq!(task.processed, 1);
    assert!(store
        .control(TaskControl {
            operation_id: "resume1".into(),
            task_id: task.id,
            state: TaskState::Requested
        })
        .is_err());
}
#[test]
fn pause_resume_and_replayed_pause_do_not_pause_again() {
    let (_temp, store, config) = setup();
    fs::write(Path::new(&config.roots[0].path).join("x.nfo"), fixture().1).unwrap();
    store.submit(request("job")).unwrap();
    let pause = TaskControl {
        operation_id: "pause1".into(),
        task_id: "job".into(),
        state: TaskState::Paused,
    };
    store.control(pause.clone()).unwrap();
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    store
        .control(TaskControl {
            operation_id: "resume1".into(),
            task_id: "job".into(),
            state: TaskState::Requested,
        })
        .unwrap();
    store.control(pause).unwrap();
    assert_eq!(store.task("job").unwrap().state, TaskState::Requested);
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("job").unwrap().state, TaskState::Completed);
}
#[test]
fn invalid_scope_and_optimistic_configuration_revision() {
    let (_temp, store, config) = setup();
    assert!(store
        .submit(ScanRequest {
            root_ids: vec![],
            ..request("empty")
        })
        .is_err());
    let mut cross = request("cross");
    cross.root_ids = vec!["tv".into()];
    assert!(store.submit(cross).is_err());
    let mut changed = config.clone();
    changed.locale = Locale::Traditional;
    store.configure("change", changed).unwrap();
    assert_eq!(
        store.configure("stale", config).unwrap_err().code,
        "configuration-conflict"
    );
}
#[test]
fn offline_root_preserves_index_and_failure_is_locatable() {
    let (temp, store, config) = setup();
    let dir = Path::new(&config.roots[0].path);
    fs::write(dir.join("one.nfo"), fixture().1).unwrap();
    store.submit(request("initial")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    fs::rename(dir, temp.path().join("offline")).unwrap();
    store.submit(request("offline")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("offline").unwrap().state, TaskState::Failed);
    let page = store.query(query(Space::Movie)).unwrap();
    assert!(page.items.iter().any(|i| i.imdb == "tt0064757"));
    assert!(page.items.iter().any(|i| i
        .error
        .as_ref()
        .is_some_and(|e| e.path.as_deref() == Some(&config.roots[0].path))));
}
#[test]
fn malformed_and_dtd_never_repair_source() {
    let (_temp, store, config) = setup();
    let dir = Path::new(&config.roots[0].path);
    for (name,raw) in [("broken.nfo","<movie>"),("entity.nfo","<!DOCTYPE movie [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><movie><title>&x;</title></movie>")] {fs::write(dir.join(name),raw).unwrap();}
    store.submit(request("errors")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("errors").unwrap().errors, 2);
    assert_eq!(
        fs::read_to_string(dir.join("broken.nfo")).unwrap(),
        "<movie>"
    );
}
#[test]
fn shutdown_is_recoverable_and_history_persists() {
    let (temp, store, _config) = setup();
    store.submit(request("stop")).unwrap();
    store.run_next(|| true, |_| {}).unwrap();
    assert_eq!(store.task("stop").unwrap().state, TaskState::Interrupted);
    drop(store);
    let reopened = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(reopened.task("stop").unwrap().state, TaskState::Interrupted);
}
#[cfg(unix)]
#[test]
fn symlink_is_rejected_instead_of_scanning_external_root() {
    let (_temp, store, config) = setup();
    std::os::unix::fs::symlink(
        &config.roots[1].path,
        Path::new(&config.roots[0].path).join("escape"),
    )
    .unwrap();
    store.submit(request("links")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("links").unwrap().errors, 1);
}
#[test]
fn future_database_is_not_downgraded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("future.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(Store::open(&path).is_err());
}

#[test]
fn database_has_one_live_owner() {
    let (temp, store, _) = setup();
    assert!(Store::open(&temp.path().join("state.sqlite")).is_err());
    drop(store);
    assert!(Store::open(&temp.path().join("state.sqlite")).is_ok());
}

#[test]
fn resumed_scan_removes_files_deleted_while_paused() {
    let (_temp, store, config) = setup();
    let path = Path::new(&config.roots[0].path).join("gone.nfo");
    fs::write(&path, fixture().1).unwrap();
    store.submit(request("resume-scan")).unwrap();
    store
        .run_next(
            || false,
            |t| {
                if t.processed == 1 {
                    store
                        .control(TaskControl {
                            operation_id: "pause-delete".into(),
                            task_id: t.id.clone(),
                            state: TaskState::Paused,
                        })
                        .unwrap();
                }
            },
        )
        .unwrap();
    assert_eq!(store.task("resume-scan").unwrap().state, TaskState::Paused);
    fs::remove_file(path).unwrap();
    store
        .control(TaskControl {
            operation_id: "resume-delete".into(),
            task_id: "resume-scan".into(),
            state: TaskState::Requested,
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 0);
    assert_eq!(store.task("resume-scan").unwrap().attempt, 2);
}

#[test]
fn long_unicode_paths_and_incremental_content_change() {
    let (_temp, store, config) = setup();
    let directory = Path::new(&config.roots[0].path)
        .join("中文目录".repeat(10))
        .join("更多目录".repeat(10));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("大写.NFO");
    fs::write(&path, fixture().1).unwrap();
    store.submit(request("long-initial")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let old = store.query(query(Space::Movie)).unwrap().items.remove(0);
    fs::write(
        &path,
        fixture().1.replace("电影 &amp; 测试", "修改后的片名"),
    )
    .unwrap();
    store.submit(request("long-refresh")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let new = store.query(query(Space::Movie)).unwrap().items.remove(0);
    assert_eq!(old.id, new.id);
    assert_ne!(old.source_hash, new.source_hash);
    assert_eq!(new.title, "修改后的片名");
}

#[test]
fn operation_results_are_queryable_and_ids_cannot_cross_operation_kinds() {
    let (temp, store, config) = setup();
    assert_eq!(
        store.operation_result("unknown").unwrap_err().code,
        "operation-not-found"
    );
    assert!(matches!(
        store.operation_result("setup").unwrap(),
        OperationResult::Configuration(_)
    ));
    assert_eq!(
        store.submit(request("setup")).unwrap_err().code,
        "operation-conflict"
    );
    store.submit(request("scan-operation")).unwrap();
    assert_eq!(
        store.configure("scan-operation", config).unwrap_err().code,
        "operation-conflict"
    );
    assert_eq!(
        store
            .control(TaskControl {
                operation_id: "scan-operation".into(),
                task_id: "scan-operation".into(),
                state: TaskState::Paused
            })
            .unwrap_err()
            .code,
        "operation-conflict"
    );
    store
        .control(TaskControl {
            operation_id: "pause-operation".into(),
            task_id: "scan-operation".into(),
            state: TaskState::Paused,
        })
        .unwrap();
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    for id in ["scan-operation", "pause-operation"] {
        let OperationResult::Task(task) = store.operation_result(id).unwrap() else {
            panic!("Expected task result")
        };
        assert_eq!(task.state, TaskState::Paused);
    }
}

#[cfg(windows)]
#[test]
fn windows_verbatim_root_and_drive_relative_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    assert_eq!(paths::checked(&root).unwrap(), root);
    let file = root.join("中文.nfo");
    fs::write(&file, "<movie/>").unwrap();
    assert_eq!(paths::within(&root, &file).unwrap(), file);
    assert_eq!(
        paths::checked(Path::new(r"C:relative")).unwrap_err().code,
        "invalid-path"
    );
    // PathBuf::join normalizes .. for verbatim Windows paths before validation.
    let mut traversal = root.as_os_str().to_os_string();
    traversal.push(r"\..");
    assert_eq!(
        paths::checked(Path::new(&traversal)).unwrap_err().code,
        "ambiguous-path"
    );
}
