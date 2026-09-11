use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tcm_core::{card_service::CardService, emby::Integration, store::Store, *};
fn setup() -> (tempfile::TempDir, Arc<Store>, LibraryRoot) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    fs::create_dir(dir.join("movies")).unwrap();
    let store = Arc::new(Store::open(&dir.join("state.sqlite")).unwrap());
    let root = LibraryRoot {
        id: "movies".into(),
        space: Space::Movie,
        path: dir.join("movies").to_str().unwrap().into(),
    };
    store
        .configure(
            "roots",
            Configuration {
                roots: vec![root.clone()],
                ..Default::default()
            },
        )
        .unwrap();
    (temp, store, root)
}
fn scan(store: &Store, id: &str) {
    store
        .submit(ScanRequest {
            operation_id: id.into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
}
fn write(root: &LibraryRoot, title: &str) {
    fs::write(Path::new(&root.path).join("movie.nfo"),format!("<movie><title>{title}</title><uniqueid type=\"imdb\">tt1234567</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>{title}</item></section></technicalspecs></movie>")).unwrap();
}
#[test]
fn unchanged_scans_skip_snapshots_and_pending_scans_hide_partial_state() {
    let (temp, store, root) = setup();
    write(&root, "Before");
    scan(&store, "first");
    let (revision, items) = store.publication_snapshot(None).unwrap().unwrap();
    assert_eq!(items.len(), 1);
    scan(&store, "same");
    assert!(store
        .publication_snapshot(Some(revision))
        .unwrap()
        .is_none());
    write(&root, "After");
    store
        .submit(ScanRequest {
            operation_id: "changed".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    store
        .run_next(
            || false,
            |task| {
                if task.processed == 1 {
                    store
                        .control(TaskControl {
                            operation_id: "pause".into(),
                            task_id: task.id.clone(),
                            state: TaskState::Paused,
                        })
                        .unwrap();
                }
            },
        )
        .unwrap();
    assert!(store.publication_snapshot(None).unwrap().is_none());
    store
        .control(TaskControl {
            operation_id: "resume".into(),
            task_id: "changed".into(),
            state: TaskState::Requested,
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let (next, items) = store.publication_snapshot(Some(revision)).unwrap().unwrap();
    assert!(next > revision);
    assert_eq!(items[0].title, "After");
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert!(store.publication_snapshot(Some(next)).unwrap().is_none());
    fs::remove_file(Path::new(&root.path).join("movie.nfo")).unwrap();
    scan(&store, "remove");
    let (deleted, items) = store.publication_snapshot(Some(next)).unwrap().unwrap();
    assert!(deleted > next);
    assert!(items.is_empty());
}
#[test]
fn live_service_publishes_changed_catalog_and_stop_joins_both_owners() {
    let (temp, store, root) = setup();
    write(&root, "Before");
    scan(&store, "first");
    let directory = temp.path().canonicalize().unwrap();
    let web = directory.join("web");
    fs::create_dir(&web).unwrap();
    fs::write(
        web.join("index.html"),
        format!(
            "<html><head><title>Emby</title></head><body>{}</body></html>",
            "Emby ".repeat(80)
        ),
    )
    .unwrap();
    let integration = Arc::new(Integration::open(&web, &directory.join("backups")).unwrap());
    let index =
        tcm_core::emby::public_index(&store.all_items().unwrap(), tcm_core::emby::timestamp());
    let plan = integration
        .plan("install", "install", b"js", &index, b"{}")
        .unwrap();
    integration.apply(&plan.id, &plan.fingerprint).unwrap();
    let mut service = CardService::start_with_store(integration, "live", store.clone()).unwrap();
    write(&root, "After");
    scan(&store, "changed");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let data = fs::read_to_string(web.join("technical-specs-data.json")).unwrap();
        if data.contains("After") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "new index was not published: {:?}",
            service.status().unwrap()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    let nfo = Path::new(&root.path).join("movie.nfo");
    let bytes = fs::read(&nfo).unwrap();
    let modified = fs::metadata(&nfo).unwrap().modified().unwrap();
    service.stop().unwrap();
    let stopped = fs::read(web.join("technical-specs-runtime.json")).unwrap();
    let data = fs::read(web.join("technical-specs-data.json")).unwrap();
    scan(&store, "post-stop");
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(
        fs::read(web.join("technical-specs-runtime.json")).unwrap(),
        stopped
    );
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
    assert_eq!(fs::read(&nfo).unwrap(), bytes);
    assert_eq!(fs::metadata(&nfo).unwrap().modified().unwrap(), modified);
}
