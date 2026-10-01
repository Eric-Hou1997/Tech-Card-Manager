use std::{fs, path::Path};
use tcm_core::{store::Store, *};

fn setup() -> (tempfile::TempDir, Store, Configuration) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::create_dir(base.join("movies")).unwrap();
    fs::create_dir(base.join("tv")).unwrap();
    let store = Store::open(&base.join("state.sqlite")).unwrap();
    let config = store
        .configure(
            "setup",
            Configuration {
                roots: vec![
                    LibraryRoot {
                        id: "movies".into(),
                        space: Space::Movie,
                        path: base.join("movies").to_string_lossy().into(),
                    },
                    LibraryRoot {
                        id: "tv".into(),
                        space: Space::Tv,
                        path: base.join("tv").to_string_lossy().into(),
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();
    (temp, store, config)
}
fn scan(store: &Store, id: &str, space: Space) {
    let root = if space == Space::Movie {
        "movies"
    } else {
        "tv"
    };
    store
        .submit(ScanRequest {
            operation_id: id.into(),
            space,
            root_ids: vec![root.into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
}
fn nfo(kind: &str, id: &str) -> String {
    format!("\u{feff}<{kind}>\r\n<title>完成</title><uniqueid type=\"imdb\">{id}</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item></section></technicalspecs></{kind}>")
}
#[test]
fn summary_distinguishes_never_generated_from_empty_and_survives_reopen() {
    let (temp, store, _) = setup();
    assert!(store.index_summary().unwrap().is_none());
    scan(&store, "empty", Space::Movie);
    let summary = store.index_summary().unwrap().unwrap();
    assert_eq!(summary.indexed_titles, 0);
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_seen, 0);
    assert_eq!(summary.scan_stats.as_ref().unwrap().online_roots_scanned, 1);
    assert_eq!(summary.source_task, "empty");
    drop(store);
    let reopened = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(reopened.index_summary().unwrap().unwrap(), summary);
}
#[test]
fn counters_measure_actual_reads_and_reparses_but_specs_cover_the_complete_catalog() {
    let (_temp, store, config) = setup();
    let movies = Path::new(&config.roots[0].path);
    let tv = Path::new(&config.roots[1].path);
    let path = movies.join("movie.nfo");
    let raw = nfo("movie", "tt1234567");
    fs::write(&path, &raw).unwrap();
    fs::write(tv.join("series.nfo"), nfo("tvshow", "tt7654321")).unwrap();
    fs::write(tv.join("episode.nfo"), nfo("episodedetails", "tt7654321")).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    scan(&store, "movie", Space::Movie);
    let first = store.index_summary().unwrap().unwrap();
    assert_eq!(first.scan_stats.as_ref().unwrap().nfo_seen, 1);
    assert_eq!(first.scan_stats.as_ref().unwrap().nfo_reparsed, 1);
    scan(&store, "tv", Space::Tv);
    let summary = store.index_summary().unwrap().unwrap();
    assert_eq!(summary.indexed_titles, 2);
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_seen, 2);
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_reparsed, 2);
    assert_eq!(
        summary.scan_stats.as_ref().unwrap().technical_specs_found,
        3
    );
    assert_eq!(
        summary
            .scan_stats
            .as_ref()
            .unwrap()
            .web_eligible_specs_found,
        2
    );
    assert_eq!(
        summary
            .scan_stats
            .as_ref()
            .unwrap()
            .episode_specs_excluded_from_web,
        1
    );
    scan(&store, "unchanged", Space::Movie);
    let cached = store.index_summary().unwrap().unwrap();
    assert_eq!(cached.scan_stats.as_ref().unwrap().nfo_seen, 1);
    assert_eq!(cached.scan_stats.as_ref().unwrap().nfo_reparsed, 0);
    assert_eq!(cached.scan_stats.as_ref().unwrap().technical_specs_found, 3);
    assert_eq!(fs::read(&path).unwrap(), raw.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fs::write(movies.join("broken.nfo"), "<movie>").unwrap();
    scan(&store, "malformed", Space::Movie);
    let bad = store.index_summary().unwrap().unwrap();
    assert_eq!(bad.scan_stats.as_ref().unwrap().nfo_seen, 2);
    assert_eq!(bad.scan_stats.as_ref().unwrap().nfo_reparsed, 1);
    assert_eq!(bad.scan_stats.as_ref().unwrap().xml_read_errors, 1);
    scan(&store, "retry-invalid", Space::Movie);
    assert_eq!(
        store
            .index_summary()
            .unwrap()
            .unwrap()
            .scan_stats
            .unwrap()
            .nfo_reparsed,
        1
    );
}
#[test]
fn cancelled_scan_and_later_directory_changes_preserve_the_previous_summary() {
    let (_temp, store, config) = setup();
    scan(&store, "complete", Space::Movie);
    let previous = store.index_summary().unwrap().unwrap();
    fs::write(
        Path::new(&config.roots[0].path).join("new.nfo"),
        nfo("movie", "tt1234567"),
    )
    .unwrap();
    store
        .submit(ScanRequest {
            operation_id: "cancel".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    let stop = std::cell::Cell::new(false);
    store
        .run_next(
            || stop.get(),
            |task| {
                if task.processed > 0 {
                    stop.set(true);
                }
            },
        )
        .unwrap();
    assert_eq!(store.index_summary().unwrap().unwrap(), previous);
    fs::remove_dir(&config.roots[1].path).unwrap();
    assert!(store.library_diagnostics().unwrap().roots[1]
        .error
        .is_some());
    assert_eq!(store.index_summary().unwrap().unwrap(), previous);
    assert!(previous.libraries[1].online);
}
#[test]
fn offline_roots_are_recorded_without_becoming_xml_read_attempts() {
    let (_temp, store, config) = setup();
    fs::remove_dir(&config.roots[1].path).unwrap();
    scan(&store, "offline", Space::Tv);
    let summary = store.index_summary().unwrap().unwrap();
    assert_eq!(summary.scan_stats.as_ref().unwrap().online_roots_scanned, 0);
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_seen, 0);
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_reparsed, 0);
    assert_eq!(summary.scan_stats.as_ref().unwrap().xml_read_errors, 0);
    assert!(
        !summary
            .libraries
            .iter()
            .find(|library| library.path == config.roots[1].path)
            .unwrap()
            .online
    );
}

fn import_summary(store: &Store, root: &Path, id: &str, kind: &str, bytes: &[u8]) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("manager-index-summary.json"), bytes).unwrap();
    let plan = store.prepare_migration(id, root, kind).unwrap();
    store.apply_migration(id, &plan.fingerprint).unwrap();
}
#[test]
fn original_summary_import_preserves_bytes_counts_and_provenance_without_claiming_new_work() {
    let (temp, store, _) = setup();
    let old = temp.path().canonicalize().unwrap().join("old");
    let value = serde_json::json!({"version":5,"generatedAt":"2026-09-12T12:00:00.0000000Z","indexedTitles":999,"items":{"tt1234567":{"Camera":["ARRI"]}},"libraryRoots":["Z:\\完成"],"libraries":[{"name":"Original","path":"Z:\\完成","kind":"Movies","online":true,"evidence":"original evidence"}],"scanStats":{"onlineRootsScanned":2,"nfoSeen":9,"nfoReparsed":3,"technicalSpecsFound":8,"webEligibleSpecsFound":7,"episodeSpecsExcludedFromWeb":1,"xmlReadErrors":2},"unknown_original_field":{"keep":"verbatim"}});
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend(serde_json::to_vec_pretty(&value).unwrap());
    bytes.extend(b"\r\n");
    import_summary(&store, &old, "original", "tcm-state", &bytes);
    let summary = store.index_summary().unwrap().unwrap();
    assert_eq!(summary.source_task, "legacy-import:original");
    assert_eq!(summary.indexed_titles, 1); // Original CheckOnly counts items, not indexedTitles.
    assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_reparsed, 3);
    assert_eq!(summary.libraries[0].path, "Z:\\完成");
    assert!(summary.libraries[0].online);
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.all_items().unwrap().is_empty());
    assert!(store.catalog_summary().unwrap().generated_at.is_none());
    assert_eq!(store.index_summary_with_source().unwrap().unwrap().1, bytes);
    store.begin_diagnostic_job("show").unwrap();
    store
        .finish_diagnostic_job(
            "show",
            &Ok(tcm_core::diagnostics::IndexDiagnostic {
                summary: Some(summary.clone()),
                frontend_ok: false,
                xml_errors_path: None,
            }),
        )
        .unwrap();
    let log = store.manager_job_log().unwrap().unwrap();
    assert!(log.contains("扫描统计：roots=2, NFO=9, reparsed=3, tech=8, webEligible=7, episodeWebSkipped=1, xmlErrors=2"));
    assert!(log.contains("[Movies] Z:\\完成  online"));
    fs::rename(&old, old.with_file_name("offline")).unwrap();
    drop(store);
    let reopened = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(reopened.index_summary().unwrap().unwrap(), summary);
    scan(&reopened, "new-empty", Space::Movie);
    let current = reopened.index_summary().unwrap().unwrap();
    assert_eq!(current.source_task, "new-empty");
    assert_eq!(current.indexed_titles, 0);
    assert_eq!(
        reopened
            .legacy_artifact("original", "manager-index-summary.json")
            .unwrap(),
        bytes
    );
}
#[test]
fn old_optional_statistics_and_plain_roots_do_not_invent_zero_measurements_or_classification() {
    let (temp, store, _) = setup();
    let old = temp.path().canonicalize().unwrap().join("old");
    import_summary(
        &store,
        &old,
        "optional",
        "tcm-state",
        br#"{"version":5,"items":{},"libraryRoots":["Z:\\offline"]}"#,
    );
    let summary = store.index_summary().unwrap().unwrap();
    assert!(summary.scan_stats.is_none());
    assert!(summary.libraries.is_empty());
    store.begin_diagnostic_job("show").unwrap();
    store
        .finish_diagnostic_job(
            "show",
            &Ok(tcm_core::diagnostics::IndexDiagnostic {
                summary: Some(summary),
                frontend_ok: false,
                xml_errors_path: None,
            }),
        )
        .unwrap();
    let log = store.manager_job_log().unwrap().unwrap();
    assert!(!log.contains("扫描统计"));
    assert!(log.contains("  Z:\\offline\n"));
    assert!(!log.contains("[Movies]"));
}
#[test]
fn unrelated_imports_conflicting_copies_and_invalid_summaries_never_become_current_state() {
    let (temp, store, _) = setup();
    let base = temp.path().canonicalize().unwrap();
    import_summary(
        &store,
        &base.join("other"),
        "other",
        "itm-engine",
        br#"{"version":5,"items":{}}"#,
    );
    assert!(store.index_summary().unwrap().is_none());
    let old = base.join("old");
    fs::create_dir_all(old.join("data")).unwrap();
    fs::write(
        old.join("data/manager-index-summary.json"),
        br#"{"version":5,"items":{"tt1234567":{}}}"#,
    )
    .unwrap();
    import_summary(
        &store,
        &old,
        "conflict",
        "tcm-state",
        br#"{"version":5,"items":{}}"#,
    );
    assert_eq!(
        store.index_summary().unwrap_err().code,
        "migration-summary-ambiguous"
    );
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.all_items().unwrap().is_empty());
    for (n, bytes) in [
        br#"{"version":7,"items":{}}"#.as_slice(),
        br#"{"version":5,"items":[]}"#,
        br#"{"version":5,"items":{},"scanStats":{"nfoSeen":-1}}"#,
        b"broken",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("invalid-{n}");
        let path = base.join(&id);
        import_summary(&store, &path, &id, "tcm-state", bytes);
        let error = store.index_summary().unwrap_err();
        assert!(error.path.is_some());
        assert_eq!(
            store
                .legacy_artifact(&id, "manager-index-summary.json")
                .unwrap(),
            bytes
        );
    }
}

fn original_pair() -> (Vec<u8>, Vec<u8>) {
    let stats = tcm_core::diagnostics::ScanStats {
        xml_read_errors: 1,
        ..Default::default()
    };
    let summary=serde_json::to_vec(&serde_json::json!({"version":5,"generatedAt":"2026-09-12T12:00:00.0000000Z","items":{},"scanStats":stats})).unwrap();
    let mut details = vec![0xef, 0xbb, 0xbf];
    details.extend_from_slice(br#"{
  "generatedAt":"2026-09-12T12:00:02.0000000Z",
  "count":1,
  "errors":[{"path":"Z:\\Media\\bad.nfo","stamp":"original-parser:41:123456789","error":"original XML failure","original_extra":"keep me"}],
  "unknown_original":"preserved"
}"#);
    details.extend_from_slice(b"\r\n");
    (summary, details)
}
#[test]
fn archived_xml_details_keep_original_bytes_and_pair_with_the_same_import() {
    let (temp, store, _) = setup();
    let root = temp.path().canonicalize().unwrap().join("original-pair");
    fs::create_dir_all(root.join("data")).unwrap();
    let (summary, details) = original_pair();
    fs::write(root.join("data/manager-index-summary.json"), &summary).unwrap();
    fs::write(root.join("data/manager-xml-errors.json"), &details).unwrap();
    let plan = store
        .prepare_migration("original-pair", &root, "tcm-portable")
        .unwrap();
    store
        .apply_migration("original-pair", &plan.fingerprint)
        .unwrap();
    let captured = store.diagnostic_index().unwrap().unwrap();
    assert_eq!(captured.summary_bytes, summary);
    let xml = captured.xml_errors.unwrap().unwrap();
    assert_eq!(xml.bytes, details);
    assert_ne!(xml.report.generated_at, captured.summary.generated_at);
    assert_eq!(xml.report.errors[0].stamp, "original-parser:41:123456789");
    assert_eq!(xml.report.errors[0].path, "Z:\\Media\\bad.nfo");
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.all_items().unwrap().is_empty());
    fs::rename(&root, root.with_file_name("offline-original")).unwrap();
    drop(store);
    let store = Store::open(&temp.path().canonicalize().unwrap().join("state.sqlite")).unwrap();
    assert_eq!(
        store
            .diagnostic_index()
            .unwrap()
            .unwrap()
            .xml_errors
            .unwrap()
            .unwrap()
            .bytes,
        details
    );
    scan(&store, "current", Space::Movie);
    let new = store.diagnostic_index().unwrap().unwrap();
    let current = new.xml_errors.unwrap().unwrap();
    assert_eq!(current.report.count, 0);
    assert_eq!(current.report.generated_at, new.summary.generated_at);
    assert_eq!(
        store
            .legacy_artifact("original-pair", "data/manager-xml-errors.json")
            .unwrap(),
        details
    );
}
#[test]
fn archived_xml_details_reject_mismatches_and_never_borrow_another_import() {
    for mode in [
        "missing",
        "bad-count",
        "old-time",
        "invalid",
        "copies",
        "corrupt",
    ] {
        let (temp, store, _) = setup();
        let base = temp.path().canonicalize().unwrap();
        let (summary, details) = original_pair();
        let unrelated = base.join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("manager-xml-errors.json"), &details).unwrap();
        let plan = store
            .prepare_migration("other", &unrelated, "tcm-state")
            .unwrap();
        store.apply_migration("other", &plan.fingerprint).unwrap();
        let original = base.join("source");
        fs::create_dir_all(original.join("data")).unwrap();
        fs::write(original.join("manager-index-summary.json"), &summary).unwrap();
        let invalid = match mode {
            "bad-count" => String::from_utf8(details[3..].to_vec())
                .unwrap()
                .replace("\"count\":1", "\"count\":2")
                .into_bytes(),
            "old-time" => String::from_utf8(details[3..].to_vec())
                .unwrap()
                .replace("12:00:02", "11:59:59")
                .into_bytes(),
            "invalid" => b"not JSON".to_vec(),
            _ => details.clone(),
        };
        if mode != "missing" {
            fs::write(original.join("manager-xml-errors.json"), &invalid).unwrap();
        }
        if mode == "copies" {
            fs::write(
                original.join("data/manager-xml-errors.json"),
                b"different copy",
            )
            .unwrap();
        }
        let plan = store
            .prepare_migration("source", &original, "tcm-state")
            .unwrap();
        store.apply_migration("source", &plan.fingerprint).unwrap();
        if mode == "corrupt" {
            let db = rusqlite::Connection::open(base.join("state.sqlite")).unwrap();
            db.execute("UPDATE legacy_artifacts SET body=?1 WHERE import_id='source' AND path='manager-xml-errors.json'",[b"corrupted archive".as_slice()]).unwrap();
        }
        let captured = store.diagnostic_index().unwrap().unwrap();
        assert_eq!(captured.summary_bytes, summary);
        assert!(captured.xml_errors.is_err(), "{mode}");
        assert!(store.index_xml_errors().is_err(), "{mode}");
        assert_eq!(
            fs::read(original.join("manager-index-summary.json")).unwrap(),
            summary
        );
        assert!(store.tasks().unwrap().is_empty());
        assert!(store.all_items().unwrap().is_empty());
    }
}
