use std::fs;
use tcm_core::store::Store;
fn setup(files: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let legacy = root.join("legacy");
    fs::create_dir(&legacy).unwrap();
    for (name, bytes) in files {
        let path = legacy.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let store = Store::open(&root.join("state.sqlite")).unwrap();
    let plan = store
        .prepare_migration("history-import", &legacy, "tcm-portable")
        .unwrap();
    store
        .apply_migration("history-import", &plan.fingerprint)
        .unwrap();
    (temp, store)
}
#[test]
fn imported_history_preserves_original_language_fields_and_never_creates_executable_tasks() {
    let value = serde_json::json!([{"job_id":"historical-active", "running":true,"action":"ai-generate","language":"ja","language_pack_revision":3,"message":"過去の記録", "started_at":"2026-08-01T00:00:00Z", "extra":{"keep":"原字段"}}]);
    let raw = serde_json::to_vec_pretty(&value).unwrap();
    let (temp, store) = setup(&[
        ("task-history.json", raw.clone()),
        ("logs/job.log", b"\xef\xbb\xbfOriginal\r\n".to_vec()),
    ]);
    let archives = store.history_archives(0, 1).unwrap();
    assert_eq!(archives.total, 2);
    assert_eq!(archives.items.len(), 1);
    let page = store
        .history_page("history-import", "task-history.json", 0)
        .unwrap();
    assert_eq!(page.unit, "records");
    assert_eq!(page.tasks[0], value[0]);
    assert!(store.tasks().unwrap().is_empty());
    assert_eq!(
        store
            .legacy_artifact("history-import", "task-history.json")
            .unwrap(),
        raw
    );
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(
        store
            .history_page("history-import", "task-history.json", 0)
            .unwrap()
            .tasks[0],
        value[0]
    );
    assert!(store.tasks().unwrap().is_empty());
}
#[test]
fn log_pagination_preserves_bom_newlines_and_utf8_without_gaps_or_repeated_bytes() {
    let text = format!("\u{feff}{}\r\n尾部", "中文日志📝\r\n".repeat(16000));
    let (_temp, store) = setup(&[("logs/job.log", text.as_bytes().to_vec())]);
    let mut offset = 0;
    let mut joined = String::new();
    loop {
        let page = store
            .history_page("history-import", "logs/job.log", offset)
            .unwrap();
        assert!(page.text.len() <= 65536);
        joined.push_str(&page.text);
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(joined, text);
    assert_eq!(
        store
            .history_page("history-import", "logs/job.log", 1)
            .unwrap_err()
            .code,
        "history-offset"
    );
}
#[test]
fn task_record_pagination_has_stable_order_and_oversized_or_unknown_json_uses_bounded_text() {
    let tasks = (0..123)
        .map(|i| serde_json::json!({"message":format!("record-{i}")}))
        .collect::<Vec<_>>();
    let (_temp, store) = setup(&[("task-history.json", serde_json::to_vec(&tasks).unwrap())]);
    let mut joined = vec![];
    let mut offset = 0;
    loop {
        let page = store
            .history_page("history-import", "task-history.json", offset)
            .unwrap();
        assert_eq!(page.unit, "records");
        joined.extend(page.tasks);
        match page.next_offset {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(joined, tasks);
    for raw in [
        b"{interrupted legacy JSON".to_vec(),
        serde_json::to_vec(&serde_json::json!([{"log":"x".repeat(70000)}])).unwrap(),
    ] {
        let (_temp, store) = setup(&[("task-history.json", raw.clone())]);
        let page = store
            .history_page("history-import", "task-history.json", 0)
            .unwrap();
        assert_eq!(page.unit, "bytes");
        assert!(page.text.len() <= 65536);
        assert_eq!(page.text.as_bytes(), &raw[..page.text.len()]);
    }
}
#[test]
fn history_view_never_exposes_other_archives_and_rejects_changed_or_non_utf8_bytes() {
    let (temp, store) = setup(&[
        ("logs/job.log", b"original".to_vec()),
        ("logs/binary.log", vec![255]),
        ("config.json", b"{}".to_vec()),
        ("cache/tt1234567.json", b"{}".to_vec()),
        ("backup/nfo.json", b"{}".to_vec()),
    ]);
    assert_eq!(store.history_archives(0, 100).unwrap().total, 2);
    for path in [
        "config.json",
        "cache/tt1234567.json",
        "backup/nfo.json",
        "../logs/job.log",
    ] {
        assert_eq!(
            store
                .history_page("history-import", path, 0)
                .unwrap_err()
                .code,
            "history-not-found"
        );
    }
    assert_eq!(
        store
            .history_page("history-import", "logs/binary.log", 0)
            .unwrap_err()
            .code,
        "history-encoding"
    );
    assert!(store.history_archives(0, 101).is_err());
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE legacy_artifacts SET body=?1 WHERE path='logs/job.log'",
        [b"changed".as_slice()],
    )
    .unwrap();
    assert_eq!(
        store
            .history_page("history-import", "logs/job.log", 0)
            .unwrap_err()
            .code,
        "history-integrity"
    );
}

#[test]
fn diagnostic_logs_use_only_original_fixed_names_and_verify_preserved_bytes() {
    let raw = b"\xff\xfeOld\r\n".to_vec();
    let (temp, store) = setup(&[
        ("logs/job.log", raw.clone()),
        ("logs/arbitrary.log", b"not exported".to_vec()),
        ("settings.json", b"{}".to_vec()),
    ]);
    let files = store.diagnostic_logs().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0.path, "logs/job.log");
    assert_eq!(files[0].1, raw);
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE legacy_artifacts SET body=x'00' WHERE path='logs/job.log'",
        [],
    )
    .unwrap();
    assert_eq!(
        store.diagnostic_logs().unwrap_err().code,
        "history-integrity"
    );
}

#[test]
fn imported_job_log_returns_to_the_idle_manager_without_rewriting_history() {
    let text = format!("\u{feff}{}\r\n中断在此", "過去の記録 📝\r\n".repeat(5000));
    let (temp, store) = setup(&[("logs/job.log", text.as_bytes().to_vec())]);
    let source = temp.path().join("legacy/logs/job.log");
    let modified = fs::metadata(&source).unwrap().modified().unwrap();
    let check = |store: &Store| {
        let job = store.manager_job().unwrap().unwrap();
        assert_eq!(job["running"], false);
        assert_eq!(job["action"], "");
        assert_eq!(job["blocks_exit"], false);
        assert!(job.get("started_at").is_none());
        let visible = job["log"].as_str().unwrap();
        assert!(visible.len() <= 40000 && text.ends_with(visible));
        assert!(visible.ends_with("中断在此"));
        assert!(store.tasks().unwrap().is_empty());
        // Export retains the archived file and its provenance, not a new job.log.
        assert!(store.manager_job_log().unwrap().is_none());
        assert_eq!(store.diagnostic_logs().unwrap()[0].1, text.as_bytes());
        assert_eq!(fs::read(&source).unwrap(), text.as_bytes());
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), modified);
    };
    check(&store);
    let mut config = store.configuration().unwrap();
    config.locale = tcm_core::Locale::English;
    store.configure("language", config).unwrap();
    check(&store);
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    check(&store);
    let media = temp.path().canonicalize().unwrap().join("movies");
    fs::create_dir(&media).unwrap();
    let mut config = store.configuration().unwrap();
    config.roots.push(tcm_core::LibraryRoot {
        id: "movies".into(),
        space: tcm_core::Space::Movie,
        path: media.display().to_string(),
    });
    store.configure("roots", config).unwrap();
    store
        .submit(tcm_core::ScanRequest {
            operation_id: "current-run".into(),
            space: tcm_core::Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    assert!(store.manager_job().unwrap().is_none());
    store.run_next(|| false, |_| {}).unwrap();
    assert!(store.manager_job().unwrap().is_none());
    assert_eq!(store.diagnostic_logs().unwrap()[0].1, text.as_bytes());
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert!(store.manager_job().unwrap().is_none());
}

#[test]
fn unrelated_or_unreadable_archives_never_become_current_job_output() {
    let (_temp, store) = setup(&[
        ("logs/manager.log", b"manager only".to_vec()),
        ("logs/arbitrary.log", b"other".to_vec()),
    ]);
    assert!(store.manager_job().unwrap().is_none());
    for (files, code) in [
        (vec![("logs/job.log", vec![255])], "history-encoding"),
        (
            vec![
                ("job.log", b"one".to_vec()),
                ("logs/job.log", b"two".to_vec()),
            ],
            "history-ambiguous",
        ),
    ] {
        let (_temp, store) = setup(&files);
        let error = store.manager_job().unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.path.as_deref(), Some("logs/job.log"));
        assert!(store.tasks().unwrap().is_empty());
    }
    let (temp, store) = setup(&[("logs/job.log", b"original".to_vec())]);
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE legacy_artifacts SET body=x'00' WHERE path='logs/job.log'",
        [],
    )
    .unwrap();
    assert_eq!(store.manager_job().unwrap_err().code, "history-integrity");
}
