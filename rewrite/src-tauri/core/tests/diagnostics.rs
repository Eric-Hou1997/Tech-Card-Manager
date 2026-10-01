use std::fs;
use tcm_core::{store::Store, *};
fn setup() -> (tempfile::TempDir, Store, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    fs::write(
        media.join("bad.nfo"),
        b"\xef\xbb\xbf<movie>\r\n<title>Broken</title>",
    )
    .unwrap();
    fs::write(media.join("good.nfo"),b"<movie><title>Private title</title><plot>Excluded private plot</plot><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>Private spec</item></section></technicalspecs></movie>").unwrap();
    let store = Store::open(&root.join("state.sqlite")).unwrap();
    store
        .configure(
            "config",
            Configuration {
                revision: 0,
                locale: Locale::English,
                roots: vec![LibraryRoot {
                    id: "movies".into(),
                    space: Space::Movie,
                    path: media.to_str().unwrap().into(),
                }],
            },
        )
        .unwrap();
    store
        .submit(ScanRequest {
            operation_id: "scan".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    (temp, store, media)
}
#[test]
fn diagnostics_reads_actual_index_and_paths_without_exporting_other_state_or_touching_nfo() {
    let (_temp, store, media) = setup();
    let original = fs::read(media.join("bad.nfo")).unwrap();
    store
        .save_preference(
            "emby-credentials",
            &serde_json::json!({"secret":"not-in-report"}),
        )
        .unwrap();
    let report = store.library_diagnostics().unwrap();
    assert_eq!(report.database, "ok");
    assert_eq!(report.total, 2);
    assert_eq!(report.errors_total, 1);
    assert!(!report.errors_truncated);
    assert_eq!(report.roots[0].state, "readable");
    assert_eq!(
        report.errors[0].path,
        media.join("bad.nfo").to_str().unwrap()
    );
    assert_eq!(report.recent_tasks[0].id, "scan");
    let json = serde_json::to_string(&report).unwrap();
    for excluded in [
        "not-in-report",
        "Excluded private plot",
        "Private spec",
        "Private title",
    ] {
        assert!(!json.contains(excluded));
    }
    assert_eq!(fs::read(media.join("bad.nfo")).unwrap(), original);
}
#[test]
fn missing_library_is_reported_while_preserving_the_index_and_actionable_path() {
    let (_temp, store, media) = setup();
    fs::rename(&media, media.with_file_name("offline")).unwrap();
    let report = store.library_diagnostics().unwrap();
    assert_eq!(report.total, 2);
    assert_eq!(report.roots[0].state, "unavailable");
    assert!(report.roots[0].error.as_ref().unwrap().path.is_some());
    assert_eq!(store.all_items().unwrap().len(), 2);
}
#[test]
fn excess_errors_are_explicitly_bounded_and_empty_workspace_is_not_a_false_failure() {
    let (temp, store, _media) = setup();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    let body: String = db
        .query_row(
            "SELECT body FROM items WHERE json_extract(body,'$.error') IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for i in 0..220 {
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        value["id"] = serde_json::json!(format!("error-{i}"));
        db.execute(
            "INSERT INTO items VALUES(?1,'movies','scan',?2)",
            rusqlite::params![format!("error-{i}"), value.to_string()],
        )
        .unwrap();
    }
    let report = store.library_diagnostics().unwrap();
    assert_eq!(report.errors_total, 221);
    assert_eq!(report.errors.len(), 200);
    assert!(report.errors_truncated);
    let empty = Store::open(&temp.path().join("empty.sqlite"))
        .unwrap()
        .library_diagnostics()
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.roots.is_empty());
    assert_eq!(empty.database, "ok");
}

#[test]
fn zip_export_preserves_raw_bytes_limits_entries_and_never_overwrites() {
    use std::io::Read;
    use tcm_core::diagnostics::{write_archive, DiagnosticFile};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("diagnostics.zip");
    let raw = b"\xff\xfe\xef\xbb\xbfraw\r\n".to_vec();
    let original_time = {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2001, 2, 3, 4, 5, 6)
            .single()
            .unwrap()
            .timestamp()
    };
    let files = vec![
        DiagnosticFile {
            name: "job.log".into(),
            bytes: raw.clone(),
            modified_unix: Some(original_time),
        },
        DiagnosticFile {
            name: "large.log".into(),
            bytes: vec![7; (12 << 20) + 1],
            modified_unix: None,
        },
    ];
    write_archive(&path, &files).unwrap();
    let before = fs::read(&path).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&before)).unwrap();
    assert_eq!(zip.len(), 2);
    let mut log = Vec::new();
    zip.by_name("job.log")
        .unwrap()
        .read_to_end(&mut log)
        .unwrap();
    assert_eq!(log, raw);
    assert_eq!(zip.by_name("large.log").unwrap().size(), 12 << 20);
    assert_eq!(
        zip.by_name("job.log").unwrap().compression(),
        zip::CompressionMethod::Deflated
    );
    let modified = zip.by_name("job.log").unwrap().last_modified().unwrap();
    assert_eq!(
        (
            modified.year(),
            modified.month(),
            modified.day(),
            modified.hour(),
            modified.minute(),
            modified.second()
        ),
        (2001, 2, 3, 4, 5, 6)
    );
    assert_eq!(
        write_archive(&path, &files).unwrap_err().code,
        "diagnostics-exists"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o077, 0);
        let link = root.join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(write_archive(&link.join("other.zip"), &files).is_err());
    }
}

#[test]
fn malformed_or_duplicate_archive_names_leave_no_partial_export() {
    use tcm_core::diagnostics::{write_archive, DiagnosticFile};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for name in [
        "../outside",
        "C:\\file",
        "/absolute",
        "a/b",
        "",
        ".",
        "..",
        "job.log",
    ] {
        let files = vec![
            DiagnosticFile {
                name: "job.log".into(),
                bytes: vec![],
                modified_unix: None,
            },
            DiagnosticFile {
                name: name.into(),
                bytes: vec![],
                modified_unix: None,
            },
        ];
        assert!(write_archive(&root.join("report.zip"), &files).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }
}

#[test]
fn completed_xml_details_are_frozen_per_scan_not_rebuilt_from_all_catalog_errors() {
    let (temp, store, media) = setup();
    let report = store.index_xml_errors().unwrap().unwrap();
    assert_eq!(report.count, 1);
    assert_eq!(
        report.errors[0].path,
        media.join("bad.nfo").to_str().unwrap()
    );
    assert_eq!(
        report.errors[0].stamp,
        format!("sha256:{}", hash(&fs::read(media.join("bad.nfo")).unwrap()))
    );
    assert_eq!(
        report.generated_at,
        store.index_summary().unwrap().unwrap().generated_at
    );
    let bytes = serde_json::to_vec(&report).unwrap();
    store
        .submit(ScanRequest {
            operation_id: "cancel-details".into(),
            space: Space::Movie,
            root_ids: vec!["movies".into()],
        })
        .unwrap();
    store.run_next(|| true, |_| {}).unwrap();
    assert_eq!(store.index_xml_errors().unwrap().unwrap(), report);
    drop(store);
    let store = Store::open(&temp.path().canonicalize().unwrap().join("state.sqlite")).unwrap();
    assert_eq!(
        serde_json::to_vec(&store.index_xml_errors().unwrap().unwrap()).unwrap(),
        bytes
    );
    store
        .control(TaskControl {
            operation_id: "cancel-old".into(),
            task_id: "cancel-details".into(),
            state: TaskState::Cancelled,
        })
        .unwrap();
    let tv = temp.path().canonicalize().unwrap().join("tv");
    fs::create_dir(&tv).unwrap();
    let mut config = store.configuration().unwrap();
    config.roots.push(LibraryRoot {
        id: "tv".into(),
        space: Space::Tv,
        path: tv.to_str().unwrap().into(),
    });
    store.configure("add-tv", config).unwrap();
    store
        .submit(ScanRequest {
            operation_id: "only-tv".into(),
            space: Space::Tv,
            root_ids: vec!["tv".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let current = store.index_xml_errors().unwrap().unwrap();
    assert_eq!(current.count, 0);
    assert!(current.errors.is_empty());
    assert_eq!(store.library_diagnostics().unwrap().errors_total, 1);
    let value = serde_json::to_value(current).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 3);
    assert!(value.get("generatedAt").is_some());
}
#[test]
fn damaged_xml_details_never_get_exported_as_the_completed_scan() {
    let (temp, store, _) = setup();
    let summary = store.index_summary().unwrap().unwrap();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute("UPDATE preferences SET body=json_set(body,'$.generatedAt','different-scan') WHERE key='manager-xml-errors'",[]).unwrap();
    assert_eq!(
        store.index_xml_errors().unwrap_err().code,
        "diagnostics-state"
    );
    assert_eq!(store.index_summary().unwrap().unwrap(), summary);
}
