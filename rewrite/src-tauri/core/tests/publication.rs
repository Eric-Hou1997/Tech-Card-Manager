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
fn read_only_index_runs_before_card_setup_and_after_emby_removes_the_patch() {
    let (temp, store, root) = setup();
    write(&root, "Before");
    let nfo = Path::new(&root.path).join("movie.nfo");
    let original_nfo = fs::read(&nfo).unwrap();
    let original_mtime = fs::metadata(&nfo).unwrap().modified().unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let web = directory.join("web");
    fs::create_dir(&web).unwrap();
    let html = format!(
        "<html><head></head><body>{}</body></html>",
        "Emby ".repeat(80)
    );
    fs::write(web.join("index.html"), &html).unwrap();
    let integration = Arc::new(Integration::open(&web, &directory.join("backups")).unwrap());
    let mut service =
        CardService::start_with_store(integration.clone(), "before-setup", store.clone()).unwrap();
    assert_eq!(service.status().unwrap().phase, "starting");
    scan(&store, "initial-read");
    let wait = |service: &CardService, title: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let status = service.status().unwrap();
            assert_ne!(status.phase, "failed", "{:?}", status.error);
            if status.phase == "running"
                && fs::read_to_string(web.join("technical-specs-data.json"))
                    .is_ok_and(|data| data.contains(title))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "index did not become ready: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait(&service, "Before");
    assert!(!integration.status().unwrap().healthy);
    assert!(!web.join("technical-specs-card.js").exists());
    assert_eq!(fs::read_to_string(web.join("index.html")).unwrap(), html);
    assert_eq!(fs::read(&nfo).unwrap(), original_nfo);
    assert_eq!(
        fs::metadata(&nfo).unwrap().modified().unwrap(),
        original_mtime
    );
    let data = fs::read(web.join("technical-specs-data.json")).unwrap();
    assert!(
        service
            .repair_web("install-while-running", b"js", b"{}")
            .unwrap()
            .healthy
    );
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
    // An Emby upgrade can remove the injection; indexing must continue without
    // automatically reinstalling the patch or replacing presentation assets.
    fs::write(web.join("index.html"), &html).unwrap();
    fs::remove_file(web.join("technical-specs-card.js")).unwrap();
    write(&root, "After");
    scan(&store, "after-upgrade");
    wait(&service, "After");
    assert_eq!(fs::read_to_string(web.join("index.html")).unwrap(), html);
    assert!(!web.join("technical-specs-card.js").exists());
    fs::remove_file(web.join("technical-specs-data.json")).unwrap();
    wait(&service, "After");
    service.stop().unwrap();
    assert!(!service.status().unwrap().lease.unwrap().enabled);
    let data = fs::read(web.join("technical-specs-data.json")).unwrap();
    let mut retry = CardService::start_with_store(integration, "retry", store).unwrap();
    wait(&retry, "After");
    retry.stop().unwrap();
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
}
#[test]
fn card_currency_requires_completed_roots_and_retains_the_last_whole_scan() {
    let (temp, store, root) = setup();
    let fingerprint = |store: &Store| {
        tcm_core::emby::index_fingerprint(&tcm_core::emby::public_index(
            &store.all_items().unwrap(),
            String::new(),
        ))
        .unwrap()
    };
    assert!(!store.index_current(Some(&fingerprint(&store))).unwrap());
    write(&root, "Before");
    scan(&store, "first");
    let original = fingerprint(&store);
    assert!(store.index_current(Some(&original)).unwrap());
    assert!(!store.index_current(None).unwrap());
    assert!(!store.index_current(Some("different-card-file")).unwrap());
    store
        .submit(ScanRequest {
            operation_id: "pending".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    assert!(store.index_current(Some(&original)).unwrap());
    store
        .control(TaskControl {
            operation_id: "cancel-pending".into(),
            task_id: "pending".into(),
            state: TaskState::Cancelled,
        })
        .unwrap();
    assert!(store.index_current(Some(&original)).unwrap());
    let empty = temp.path().canonicalize().unwrap().join("empty");
    fs::create_dir(&empty).unwrap();
    let mut configuration = store.configuration().unwrap();
    configuration.roots.push(LibraryRoot {
        id: "empty".into(),
        space: Space::Movie,
        path: empty.to_str().unwrap().into(),
    });
    store.configure("extra-root", configuration).unwrap();
    assert_eq!(
        fingerprint(&store),
        original,
        "adding an empty root does not change card contents"
    );
    assert!(!store.index_current(Some(&original)).unwrap());
    store
        .submit(ScanRequest {
            operation_id: "all-roots".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into(), "empty".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert!(store.index_current(Some(&original)).unwrap());
    let mut configuration = store.configuration().unwrap();
    configuration.locale = Locale::English;
    store.configure("language", configuration).unwrap();
    assert!(store.index_current(Some(&original)).unwrap());
    drop(store);
    let store = Store::open(&temp.path().canonicalize().unwrap().join("state.sqlite")).unwrap();
    assert!(store.index_current(Some(&original)).unwrap());
}
#[test]
fn a_failed_summary_commit_cannot_replace_completed_card_identity() {
    let (temp, store, root) = setup();
    write(&root, "Before");
    scan(&store, "first");
    let fingerprint = tcm_core::emby::index_fingerprint(&tcm_core::emby::public_index(
        &store.all_items().unwrap(),
        String::new(),
    ))
    .unwrap();
    let completed = store.preferences("manager-completed-publication").unwrap();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_xml BEFORE UPDATE ON preferences WHEN NEW.key='manager-xml-errors' BEGIN SELECT RAISE(ABORT,'fixture summary write rejected'); END;").unwrap();
    write(&root, "After");
    store
        .submit(ScanRequest {
            operation_id: "changed".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    assert!(store.run_next(|| false, |_| {}).is_err());
    assert_eq!(
        store.preferences("manager-completed-publication").unwrap(),
        completed
    );
    assert!(store.index_current(Some(&fingerprint)).unwrap());
    db.execute(
        "UPDATE preferences SET body='{invalid' WHERE key='manager-completed-publication'",
        [],
    )
    .unwrap();
    assert!(store.index_current(Some(&fingerprint)).is_err());
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
fn display_lease_waits_for_completed_current_roots_and_recovers_after_rebuild() {
    let (temp, store, root) = setup();
    write(&root, "Before");
    let directory = temp.path().canonicalize().unwrap();
    let web = directory.join("web");
    fs::create_dir(&web).unwrap();
    fs::write(
        web.join("index.html"),
        format!(
            "<html><head></head><body>{}</body></html>",
            "Emby ".repeat(80)
        ),
    )
    .unwrap();
    let integration = Arc::new(Integration::open(&web, &directory.join("backups")).unwrap());
    let plan = integration
        .plan(
            "install",
            "install",
            b"js",
            &tcm_core::emby::public_index(&[], String::new()),
            b"{}",
        )
        .unwrap();
    integration.apply(&plan.id, &plan.fingerprint).unwrap();
    let mut service = CardService::start_with_store(integration, "waiting", store.clone()).unwrap();
    assert_eq!(service.status().unwrap().phase, "starting");
    assert!(service.status().unwrap().last_started_at.is_none());
    assert!(!service.status().unwrap().lease.unwrap().enabled);
    let lease = || {
        serde_json::from_slice::<tcm_core::emby::Lease>(
            &fs::read(web.join("technical-specs-runtime.json")).unwrap(),
        )
        .unwrap()
    };
    let wait = |enabled| {
        let deadline = Instant::now() + Duration::from_secs(7);
        while lease().enabled != enabled
            || (enabled && service.status().unwrap().phase != "running")
        {
            assert!(
                Instant::now() < deadline,
                "lease did not reach {enabled}: {:?}",
                service.status().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    assert!(!lease().enabled);
    scan(&store, "first");
    wait(true);
    assert_eq!(service.status().unwrap().phase, "running");
    assert!(service.status().unwrap().last_started_at.is_some());
    assert!(fs::read_to_string(web.join("technical-specs-data.json"))
        .unwrap()
        .contains("Before"));
    let empty = directory.join("empty");
    fs::create_dir(&empty).unwrap();
    let mut config = store.configuration().unwrap();
    config.roots.push(LibraryRoot {
        id: "empty".into(),
        space: Space::Movie,
        path: empty.to_str().unwrap().into(),
    });
    store.configure("new-root", config).unwrap();
    wait(false);
    store
        .submit(ScanRequest {
            operation_id: "complete-new-roots".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into(), "empty".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    wait(true);
    service.stop().unwrap();
    assert!(!lease().enabled);
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
