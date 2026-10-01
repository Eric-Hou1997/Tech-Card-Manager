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
fn setup(count: usize) -> (tempfile::TempDir, Arc<Store>) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let store = Arc::new(Store::open(&dir.join("state.sqlite")).unwrap());
    let mut roots = vec![];
    for (name, space, kind) in [
        ("movies", Space::Movie, "movie"),
        ("tv", Space::Tv, "tvshow"),
    ] {
        let path = dir.join(name);
        fs::create_dir(&path).unwrap();
        for n in 0..count {
            fs::write(
                path.join(format!("{n}.nfo")),
                format!("\u{feff}<{kind}>\r\n<title>Original {n}</title></{kind}>"),
            )
            .unwrap();
        }
        roots.push(LibraryRoot {
            id: name.into(),
            space,
            path: path.to_string_lossy().into(),
        });
    }
    store
        .configure(
            "configure",
            Configuration {
                roots,
                ..Default::default()
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
    (temp, store)
}
fn wait(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "worker timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn explicit_rebuild_reparses_both_spaces_immediately_and_leaves_source_unchanged() {
    let (temp, store) = setup(1);
    let mut before = vec![];
    for root in store.configuration().unwrap().roots {
        let path = std::path::Path::new(&root.path).join("0.nfo");
        before.push((
            path.clone(),
            fs::read(&path).unwrap(),
            fs::metadata(&path).unwrap().modified().unwrap(),
        ));
        store
            .submit(ScanRequest {
                operation_id: format!("scan-{}", root.id),
                space: root.space,
                root_ids: vec![root.id],
            })
            .unwrap();
        store.run_next(|| false, |_| {}).unwrap();
    }
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE items SET body=json_set(body,'$.title','POISONED CACHE')",
        [],
    )
    .unwrap();
    drop(db);
    let mut worker =
        IncrementalWorker::start(store.clone(), "session", || Ok(true), |_| {}).unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    assert!(store
        .all_items()
        .unwrap()
        .iter()
        .all(|item| item.title == "POISONED CACHE"));
    let tasks = store.submit_rebuild("rebuild", 1, "session").unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| task.force_parse));
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    wait(|| {
        tasks
            .iter()
            .all(|task| store.task(&task.id).unwrap().state == TaskState::Completed)
    });
    assert!(store
        .all_items()
        .unwrap()
        .iter()
        .all(|item| item.title == "Original 0"));
    assert_eq!(
        store.submit_rebuild("rebuild", 1, "session").unwrap(),
        tasks
    );
    worker.stop().unwrap();
    drop(worker);
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(
        store.submit_rebuild("rebuild", 1, "session").unwrap(),
        tasks
    );
    for (path, bytes, modified) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }
}
#[test]
fn rebuild_rejects_stale_busy_and_colliding_requests_without_partial_queue() {
    let (_temp, store) = setup(1);
    assert_eq!(
        store
            .submit_rebuild("stale", 0, "session")
            .unwrap_err()
            .code,
        "configuration-conflict"
    );
    assert!(store.tasks().unwrap().is_empty());
    let child = format!(
        "rebuild-{}",
        hash(&serde_json::to_vec(&("conflict", &Space::Tv)).unwrap())
    );
    store.save_ui_state(&child, Default::default()).unwrap();
    assert_eq!(
        store
            .submit_rebuild("conflict", 1, "session")
            .unwrap_err()
            .code,
        "operation-conflict"
    );
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.operation_result("conflict").is_err());
    let tasks = store.submit_rebuild("good", 1, "session").unwrap();
    assert_eq!(
        store.submit_rebuild("busy", 1, "session").unwrap_err().code,
        "task-busy"
    );
    assert_eq!(store.tasks().unwrap().len(), tasks.len());
    assert_eq!(
        store
            .submit_rebuild("good", 1, "other-session")
            .unwrap_err()
            .code,
        "operation-conflict"
    );
}
#[test]
fn stopping_service_cancels_running_and_queued_rebuild_tasks_before_returning() {
    let (_temp, store) = setup(100);
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let mut worker = IncrementalWorker::start(
        store.clone(),
        "session",
        || Ok(true),
        move |task| {
            if task.force_parse && task.processed == 1 {
                flag.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(150));
            }
        },
    )
    .unwrap();
    wait(|| worker.status().unwrap().completed_cycles == 1);
    let tasks = store.submit_rebuild("cancel", 1, "session").unwrap();
    wait(|| entered.load(Ordering::SeqCst));
    worker.stop().unwrap();
    let after: Vec<_> = tasks
        .iter()
        .map(|task| store.task(&task.id).unwrap())
        .collect();
    assert!(after.iter().all(|task| task.state == TaskState::Cancelled));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        after,
        tasks
            .iter()
            .map(|task| store.task(&task.id).unwrap())
            .collect::<Vec<_>>()
    );
}
#[test]
fn database_v4_tasks_remain_readable_but_new_rebuild_semantics_prevent_downgrade() {
    let (temp, store) = setup(1);
    drop(store);
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.pragma_update(None, "user_version", 4).unwrap();
    drop(db);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(store.configuration().unwrap().roots.len(), 2);
    drop(store);
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        7
    );
}

#[test]
fn offline_root_blocks_the_entire_rebuild_before_any_task_or_index_change() {
    let (temp, store) = setup(1);
    let root = temp.path().canonicalize().unwrap();
    fs::rename(root.join("tv"), root.join("tv-offline")).unwrap();
    assert!(store.submit_rebuild("offline", 1, "session").is_err());
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.all_items().unwrap().is_empty());
    assert!(store.operation_result("offline").is_err());
    fs::rename(root.join("tv-offline"), root.join("tv")).unwrap();
    assert_eq!(
        store.submit_rebuild("offline", 1, "session").unwrap().len(),
        2
    );
}
