use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tcm_core::{
    incremental::{IncrementalSettings, IncrementalWorker},
    store::Store,
    *,
};
fn setup() -> (tempfile::TempDir, Arc<Store>) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("movies")).unwrap();
    let store = Arc::new(Store::open(&root.join("state.sqlite")).unwrap());
    store
        .configure(
            "roots",
            Configuration {
                roots: vec![LibraryRoot {
                    id: "movies".into(),
                    space: Space::Movie,
                    path: root.join("movies").display().to_string(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    (temp, store)
}
fn wait(mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < until, "worker timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn diagnostic_winning_scan_submission_race_does_not_fail_the_service_reader() {
    let (_temp, store) = setup();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let active_calls = calls.clone();
    let active_store = store.clone();
    let mut worker = IncrementalWorker::start(
        store.clone(),
        "diagnostic-race",
        move || {
            // First callback is the loop preflight; the second is immediately
            // before submitting the first space, after the idle-job observation.
            if active_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                active_store.begin_diagnostic_job("diagnostic")?;
            }
            Ok(true)
        },
        |_| {},
    )
    .unwrap();
    wait(|| calls.load(Ordering::SeqCst) > 2);
    assert_eq!(worker.status().unwrap().phase, "waiting");
    assert!(store.tasks().unwrap().is_empty());
    store
        .finish_diagnostic_job(
            "diagnostic",
            &Ok(tcm_core::diagnostics::IndexDiagnostic {
                summary: None,
                frontend_ok: false,
                xml_errors_path: None,
            }),
        )
        .unwrap();
    wait(|| worker.status().unwrap().completed_cycles > 0);
    assert_eq!(worker.status().unwrap().completed_cycles, 1);
    assert!(worker.status().unwrap().error.is_none());
    assert!(!store.tasks().unwrap().is_empty());
    worker.stop().unwrap();
    assert_eq!(worker.status().unwrap().phase, "stopped");
}
#[test]
fn settings_are_bounded_transactional_and_persisted() {
    let (temp, store) = setup();
    let value = IncrementalSettings {
        revision: 0,
        interval_seconds: 30,
    };
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 60);
    assert_eq!(
        store
            .save_incremental_settings("save", value.clone())
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        store
            .save_incremental_settings("save", value.clone())
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        store
            .save_incremental_settings("stale", value)
            .unwrap_err()
            .code,
        "incremental-settings-conflict"
    );
    for seconds in [0, 29, 86401, u32::MAX] {
        assert!(store
            .save_incremental_settings(
                "invalid",
                IncrementalSettings {
                    revision: 1,
                    interval_seconds: seconds
                }
            )
            .is_err());
    }
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 30);
    store
        .save_preference("incremental-settings", &serde_json::json!({"broken":true}))
        .unwrap();
    assert!(store.incremental_settings().is_err());
}
#[test]
fn service_reads_immediately_and_stops_without_touching_nfo() {
    let (temp, store) = setup();
    let path = temp.path().join("movies/movie.nfo");
    let bytes = b"\xef\xbb\xbf<movie><title>Before</title></movie>\r\n";
    fs::write(&path, bytes).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let mut worker =
        IncrementalWorker::start(store.clone(), "session-one", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    assert_eq!(store.all_items().unwrap()[0].title, "Before");
    let task = store.tasks().unwrap().pop().unwrap();
    assert_eq!(task.service_session.as_deref(), Some("session-one"));
    assert_eq!(
        store
            .control(TaskControl {
                operation_id: "manual-control".into(),
                task_id: task.id,
                state: TaskState::Requested
            })
            .unwrap_err()
            .code,
        "service-owned-task"
    );
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    assert_eq!(worker.stop().unwrap().phase, "stopped");
    assert_eq!(worker.stop().unwrap().phase, "stopped");
    assert_eq!(fs::read(path.clone()).unwrap(), bytes);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), before);
}

#[test]
fn saved_folders_refresh_immediately_on_the_existing_service_reader() {
    let (temp, store) = setup();
    store
        .save_incremental_settings(
            "long-interval",
            IncrementalSettings {
                revision: 0,
                interval_seconds: 3600,
            },
        )
        .unwrap();
    let mut worker =
        IncrementalWorker::start(store.clone(), "saved-folders", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    let mut settings = store.folder_settings().unwrap();
    let tv = temp.path().canonicalize().unwrap().join("tv");
    fs::create_dir(&tv).unwrap();
    settings.folders.push(tcm_core::folders::MediaFolder {
        id: "tv".into(),
        path: tv.display().to_string(),
        name: "TV".into(),
        kind: tcm_core::folders::FolderKind::Tv,
        source: tcm_core::folders::FolderSource::Manual,
        enabled: true,
    });
    let saved = store
        .save_folder_settings("save-more-folders", settings)
        .unwrap();
    let mut before = vec![];
    for (path, bytes) in [
        (
            temp.path().join("movies/movie.nfo"),
            b"\xef\xbb\xbf<movie><title>Movie</title></movie>\r\n".as_slice(),
        ),
        (
            tv.join("tvshow.nfo"),
            b"<tvshow><title>Series</title></tvshow>\n".as_slice(),
        ),
    ] {
        fs::write(&path, bytes).unwrap();
        before.push((
            path.clone(),
            bytes.to_vec(),
            fs::metadata(&path).unwrap().modified().unwrap(),
        ));
    }
    assert_eq!(
        worker
            .request_refresh(saved.configuration.revision - 1)
            .unwrap_err()
            .code,
        "configuration-conflict"
    );
    worker
        .request_refresh(saved.configuration.revision)
        .unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 2);
    assert_eq!(store.all_items().unwrap().len(), 2);
    let tasks: Vec<_> = store
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|task| task.id.starts_with("refresh-"))
        .collect();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| !task.force_parse
        && task.service_session.as_deref() == Some("saved-folders")
        && task.state == TaskState::Completed));
    worker.stop().unwrap();
    assert_eq!(
        worker
            .request_refresh(saved.configuration.revision)
            .unwrap_err()
            .code,
        "service-stopped"
    );
    for (path, bytes, modified) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }
}

#[test]
fn explicit_refresh_rejects_a_running_job_without_queueing_another_cycle() {
    let (temp, store) = setup();
    fs::write(
        temp.path().join("movies/movie.nfo"),
        b"<movie><title>Movie</title></movie>",
    )
    .unwrap();
    let (entered, receiving) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let mut worker = IncrementalWorker::start(
        store.clone(),
        "busy-refresh",
        || Ok(true),
        move |task| {
            if task.state == TaskState::Running && task.processed == 1 {
                entered.send(()).unwrap();
                resume.recv().unwrap();
            }
        },
    )
    .unwrap();
    receiving.recv_timeout(Duration::from_secs(5)).unwrap();
    let result = worker.request_refresh(store.configuration().unwrap().revision);
    release.send(()).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    worker.stop().unwrap();
    assert_eq!(result.unwrap_err().code, "task-busy");
    assert_eq!(store.tasks().unwrap().len(), 1);
}
#[test]
fn manual_work_is_preserved_and_blocks_automatic_checks() {
    let (_temp, store) = setup();
    store
        .submit(ScanRequest {
            operation_id: "manual".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    let mut worker =
        IncrementalWorker::start(store.clone(), "service", || Ok(true), |_| {}).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(
        worker
            .request_refresh(store.configuration().unwrap().revision)
            .unwrap_err()
            .code,
        "task-busy"
    );
    assert_eq!(store.tasks().unwrap().len(), 1);
    worker.stop().unwrap();
    assert_eq!(store.task("manual").unwrap().state, TaskState::Requested);
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("manual").unwrap().state, TaskState::Completed);
}
#[test]
fn inactive_and_failed_service_never_scan() {
    let (_temp, store) = setup();
    let mut worker =
        IncrementalWorker::start(store.clone(), "inactive", || Ok(false), |_| {}).unwrap();
    wait(|| worker.status().unwrap().phase == "stopped");
    worker.stop().unwrap();
    assert!(store.tasks().unwrap().is_empty());
    let mut worker = IncrementalWorker::start(
        store.clone(),
        "failed",
        || Err(AppError::new("probe-failed", "test")),
        |_| {},
    )
    .unwrap();
    wait(|| worker.status().unwrap().phase == "failed");
    assert_eq!(worker.stop().unwrap().error.unwrap().code, "probe-failed");
    assert!(store.tasks().unwrap().is_empty());
}
#[test]
fn stop_joins_inflight_reader_and_reopen_cannot_resume_it() {
    let (temp, store) = setup();
    for i in 0..20 {
        fs::write(
            temp.path().join(format!("movies/{i}.nfo")),
            "<movie><title>Test</title></movie>",
        )
        .unwrap();
    }
    let entered = Arc::new(AtomicBool::new(false));
    let seen = entered.clone();
    let mut worker = IncrementalWorker::start(
        store.clone(),
        "cancel-me",
        || Ok(true),
        move |task| {
            if task.processed == 1 {
                seen.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(150));
            }
        },
    )
    .unwrap();
    wait(|| entered.load(Ordering::SeqCst));
    worker.stop().unwrap();
    let tasks = store.tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].state, TaskState::Cancelled);
    assert_eq!(tasks[0].processed, 1);
    drop(worker);
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(store.tasks().unwrap()[0].state, TaskState::Cancelled);
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
}

#[test]
fn stop_retries_persisted_scan_cleanup_after_the_reader_was_joined() {
    let (temp, store) = setup();
    store
        .submit(ScanRequest {
            operation_id: "cleanup-seed".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    store.freeze_for_update().unwrap();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE tasks SET body=json_set(body,'$.state','requested','$.service_session','retry-cleanup') WHERE id='cleanup-seed'",
        [],
    )
    .unwrap();
    drop(db);

    let mut worker =
        IncrementalWorker::start(store.clone(), "retry-cleanup", || Ok(false), |_| {}).unwrap();
    assert_eq!(worker.stop().unwrap_err().code, "update-in-progress");
    assert_eq!(
        store.task("cleanup-seed").unwrap().state,
        TaskState::Requested
    );

    store.unfreeze_after_update();
    worker.stop().unwrap();
    assert_eq!(
        store.task("cleanup-seed").unwrap().state,
        TaskState::Cancelled
    );
}

#[test]
fn legacy_interval_is_reviewed_and_changed_destination_rolls_back_import() {
    let (temp, store) = setup();
    let old = temp.path().canonicalize().unwrap().join("old");
    fs::create_dir(&old).unwrap();
    let bytes = br#"{"interval_seconds":300,"running":true}"#;
    fs::write(old.join("settings.json"), bytes).unwrap();
    let plan = store
        .prepare_migration("import", &old, "tcm-portable")
        .unwrap();
    assert_eq!(plan.incremental.as_ref().unwrap().interval_seconds, 300);
    store
        .save_incremental_settings(
            "edit",
            IncrementalSettings {
                revision: 0,
                interval_seconds: 900,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .apply_migration("import", &plan.fingerprint)
            .unwrap_err()
            .code,
        "migration-settings-conflict"
    );
    assert!(store.legacy_artifact("import", "settings.json").is_err());
    let plan = store
        .prepare_migration("retry", &old, "tcm-portable")
        .unwrap();
    store.apply_migration("retry", &plan.fingerprint).unwrap();
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 300);
    assert!(store.tasks().unwrap().is_empty());
    assert_eq!(
        store.legacy_artifact("retry", "settings.json").unwrap(),
        bytes
    );
}
#[test]
fn conflicting_or_invalid_legacy_intervals_require_resolution() {
    let (temp, store) = setup();
    let old = temp.path().canonicalize().unwrap().join("old");
    fs::create_dir(&old).unwrap();
    for raw in ["true", "29", "86401", "1.5", "\"60\""] {
        fs::write(
            old.join("settings.json"),
            format!("{{\"interval_seconds\":{raw}}}"),
        )
        .unwrap();
        assert!(store
            .prepare_migration("invalid", &old, "tcm-portable")
            .is_err());
    }
    fs::write(old.join("settings.json"), r#"{"interval_seconds":60}"#).unwrap();
    fs::write(old.join("config.json"), r#"{"interval_seconds":300}"#).unwrap();
    assert!(store
        .prepare_migration("ambiguous", &old, "tcm-portable")
        .is_err());
}
#[test]
fn next_timed_cycle_observes_changed_nfo_and_stop_prevents_more_reads() {
    let (temp, store) = setup();
    let path = temp.path().join("movies/movie.nfo");
    fs::write(&path, "<movie><title>Before</title></movie>").unwrap();
    store
        .save_incremental_settings(
            "interval",
            IncrementalSettings {
                revision: 0,
                interval_seconds: 30,
            },
        )
        .unwrap();
    let mut worker =
        IncrementalWorker::start(store.clone(), "periodic", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    fs::write(&path, "<movie><title>After</title></movie>").unwrap();
    let until = Instant::now() + Duration::from_secs(35);
    while worker.status().unwrap().completed_cycles < 2 {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(store.all_items().unwrap()[0].title, "After");
    worker.stop().unwrap();
    fs::write(&path, "<movie><title>Stopped</title></movie>").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(store.all_items().unwrap()[0].title, "After");
    assert_eq!(store.tasks().unwrap().len(), 2);
}
#[test]
fn completed_check_surfaces_read_errors_and_crashed_owner_is_cancelled_on_reopen() {
    let (temp, store) = setup();
    fs::write(temp.path().join("movies/broken.nfo"), "<movie>").unwrap();
    let mut worker =
        IncrementalWorker::start(store.clone(), "errors", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    assert_eq!(worker.status().unwrap().last_errors, 1);
    worker.stop().unwrap();
    let task = store.tasks().unwrap().pop().unwrap();
    drop(worker);
    drop(store);
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE tasks SET body=json_set(body,'$.state','running') WHERE id=?1",
        [&task.id],
    )
    .unwrap();
    drop(db);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(store.task(&task.id).unwrap().state, TaskState::Cancelled);
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
}
#[test]
fn large_completed_history_keeps_fifo_and_does_not_become_executable() {
    let (temp, store) = setup();
    let request = |id: &str| ScanRequest {
        operation_id: id.into(),
        space: Space::Movie,
        root_ids: vec!["movies".into()],
    };
    store.submit(request("seed")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let seed = store.task("seed").unwrap();
    let mut db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    let tx = db.transaction().unwrap();
    for i in 0..5000 {
        let mut task = seed.clone();
        task.id = format!("history-{i}");
        task.service_session = Some("old-service".into());
        tx.execute(
            "INSERT INTO tasks VALUES(?1,?2,?3)",
            rusqlite::params![
                task.id,
                format!("history-{i}"),
                serde_json::to_string(&task).unwrap()
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    store.submit(request("first")).unwrap();
    store.submit(request("second")).unwrap();
    assert_eq!(
        store.run_next(|| false, |_| {}).unwrap().unwrap().id,
        "first"
    );
    assert_eq!(
        store.run_next(|| false, |_| {}).unwrap().unwrap().id,
        "second"
    );
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    assert_eq!(store.tasks().unwrap().len(), 5003);
}
