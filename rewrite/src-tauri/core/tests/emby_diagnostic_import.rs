use std::{
    fs,
    path::{Path, PathBuf},
};
use tcm_core::{store::Store, *};
fn setup() -> (tempfile::TempDir, Store, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let roaming = base.join("roaming");
    let web = roaming.join("Emby-Server/system/dashboard-ui");
    let source = roaming.join("Emby-Server/programdata/custom-tech-specs");
    fs::create_dir_all(&web).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("technical-specs-worker.ps1"),
        include_bytes!("../../../../windows/engine/windows-engine.ps1"),
    )
    .unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        br#"{"version":5,"generatedAt":"2026-09-12T12:00:00Z","items":{}}"#,
    )
    .unwrap();
    fs::write(
        source.join("manager-xml-errors.json"),
        b"\xef\xbb\xbf{\"generatedAt\":\"2026-09-12T12:00:01Z\",\"count\":0,\"errors\":[]}\r\n",
    )
    .unwrap();
    let store = Store::open(&base.join("state.sqlite")).unwrap();
    (temp, store, roaming, web, source)
}
fn capture(
    store: &Store,
    web: &Path,
    roaming: &Path,
) -> Result<Option<tcm_core::migration::MigrationReceipt>> {
    // Portable tests of the Windows source policy; no Windows process executes.
    store.import_original_emby_diagnostics("windows", "org.tcm.product", web, Some(roaming))
}
#[test]
fn only_original_diagnostics_are_archived_without_changing_choices_or_source_and_receipt_survives_reopen(
) {
    let (temp, store, roaming, web, source) = setup();
    fs::write(
        source.join("manager-items-cache.json"),
        vec![b' '; 1024 * 1024],
    )
    .unwrap();
    fs::write(
        source.join("settings.json"),
        br#"{"api_key":"not-part-of-diagnostics"}"#,
    )
    .unwrap();
    let before: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.clone(),
                fs::read(&path).unwrap(),
                fs::metadata(path).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let config = store.configuration().unwrap();
    let summary_modified: i64 = fs::metadata(source.join("manager-index-summary.json"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
    let xml_modified: i64 = fs::metadata(source.join("manager-xml-errors.json"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
    let receipt = capture(&store, &web, &roaming).unwrap().unwrap();
    assert_eq!(receipt.imported_files, 2);
    let total = fs::metadata(source.join("manager-index-summary.json"))
        .unwrap()
        .len()
        + fs::metadata(source.join("manager-xml-errors.json"))
            .unwrap()
            .len();
    assert_eq!(receipt.imported_bytes, total.to_string());
    assert_eq!(store.configuration().unwrap(), config);
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.all_items().unwrap().is_empty());
    assert!(store
        .legacy_artifact(&receipt.id, "manager-items-cache.json")
        .is_err());
    assert!(store
        .legacy_artifact(&receipt.id, "technical-specs-worker.ps1")
        .is_err());
    let snapshot = store.diagnostic_index().unwrap().unwrap();
    assert_eq!(snapshot.summary_modified_unix, Some(summary_modified));
    let xml = snapshot.xml_errors.unwrap().unwrap();
    assert_eq!(xml.modified_unix, Some(xml_modified));
    assert_eq!(
        xml.bytes,
        fs::read(source.join("manager-xml-errors.json")).unwrap()
    );
    for (path, bytes, modified) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
    fs::rename(&source, source.with_file_name("offline")).unwrap();
    drop(store);
    let store = Store::open(&temp.path().canonicalize().unwrap().join("state.sqlite")).unwrap();
    assert_eq!(
        capture(&store, &web, &roaming).unwrap().unwrap().id,
        receipt.id
    );
    assert!(store
        .diagnostic_index()
        .unwrap()
        .unwrap()
        .xml_errors
        .unwrap()
        .is_some());
}
#[test]
fn validation_other_platforms_and_other_installations_do_not_probe_or_import_original_state() {
    let (_temp, store, roaming, web, source) = setup();
    fs::write(source.join("technical-specs-worker.ps1"), b"unrecognized").unwrap();
    for (os, id) in [
        ("windows", "org.tcm.validation"),
        ("macos", "org.tcm.product"),
        ("linux", "org.tcm.product"),
    ] {
        assert!(store
            .import_original_emby_diagnostics(os, id, &web, Some(&roaming))
            .unwrap()
            .is_none());
    }
    let other = roaming.join("Different-Emby/system/dashboard-ui");
    fs::create_dir_all(&other).unwrap();
    assert!(capture(&store, &other, &roaming).unwrap().is_none());
    assert_eq!(
        capture(&store, &web, &roaming).unwrap_err().code,
        "migration-source-unverified"
    );
    assert!(store.index_summary().unwrap().is_none());
}
#[test]
fn current_scan_wins_without_reopening_old_data() {
    let (_temp, store, roaming, web, source) = setup();
    store
        .configure(
            "roots",
            Configuration {
                roots: vec![LibraryRoot {
                    id: "media".into(),
                    space: Space::Movie,
                    path: web.to_str().unwrap().into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    store
        .submit(ScanRequest {
            operation_id: "scan".into(),
            space: Space::Movie,
            root_ids: vec!["media".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    fs::write(source.join("technical-specs-worker.ps1"), b"unrecognized").unwrap();
    assert!(capture(&store, &web, &roaming).unwrap().is_none());
    assert_eq!(store.index_summary().unwrap().unwrap().source_task, "scan");
    #[cfg(unix)]
    {
        let moved = source.with_file_name("offline-original");
        fs::rename(&source, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &source).unwrap();
        assert!(capture(&store, &web, &roaming).unwrap().is_none());
    }
}
#[test]
fn archive_failure_rolls_back_both_files_and_receipt_and_can_retry() {
    let (temp, store, roaming, web, source) = setup();
    let media = temp.path().canonicalize().unwrap().join("recovered media");
    fs::create_dir(&media).unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "generatedAt": "2026-09-12T12:00:00Z",
            "scanStats": {},
            "items": {},
            "libraries": [{"name": "Recovered", "path": media, "kind": "Movies"}]
        }))
        .unwrap(),
    )
    .unwrap();
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_detail BEFORE INSERT ON legacy_artifacts WHEN NEW.path='manager-xml-errors.json' BEGIN SELECT RAISE(ABORT,'detail rejected'); END;").unwrap();
    assert!(capture(&store, &web, &roaming).is_err());
    for table in ["legacy_artifacts", "operations"] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    assert!(store.index_summary().unwrap().is_none());
    assert_eq!(store.configuration().unwrap().revision, 0);
    assert!(store.folder_settings().unwrap().folders.is_empty());
    db.execute_batch("DROP TRIGGER reject_detail").unwrap();
    assert_eq!(
        capture(&store, &web, &roaming)
            .unwrap()
            .unwrap()
            .imported_files,
        2
    );
    assert_eq!(store.configuration().unwrap().roots.len(), 1);
    assert_eq!(
        store.folder_settings().unwrap().folders[0].name,
        "Recovered"
    );
}

#[test]
fn complete_original_summary_recovers_missing_directory_settings_and_catalog_together() {
    let (temp, store, roaming, web, source) = setup();
    let base = temp.path().canonicalize().unwrap();
    let media = base.join("old movies");
    let nfo = media.join("Film/movie.nfo");
    fs::create_dir(&media).unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 5,
            "generatedAt": "2026-09-12T12:00:00Z",
            "scanStats": {},
            "items": {},
            "libraries": [{"name": "电影库", "path": media, "kind": "Movie Library"}],
            "libraryRoots": [media],
            "catalogCount": 1
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("manager-catalog.json"),
        serde_json::to_vec(&serde_json::json!({
            "generatedAt": "2026-09-12T12:00:01Z",
            "count": 1,
            "items": [{
                "path": nfo,
                "type": "Movie",
                "title": "Film",
                "libraryKind": "movies",
                "hasTechnicalSpecs": false,
                "tagCount": 0,
                "tags": [],
                "specs": {},
                "error": ""
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = capture(&store, &web, &roaming).unwrap().unwrap();
    assert_eq!(receipt.phase, "imported-with-readonly-catalog");
    assert_eq!(receipt.configuration.revision, 1);
    assert_eq!(receipt.configuration.roots.len(), 1);
    assert_eq!(receipt.configuration.roots[0].space, Space::Movie);
    let folders = store.folder_settings().unwrap();
    assert_eq!(folders.revision, 1);
    assert_eq!(folders.folders.len(), 1);
    assert_eq!(folders.folders[0].name, "电影库");
    assert_eq!(
        folders.folders[0].kind,
        tcm_core::folders::FolderKind::Movies
    );
    assert_eq!(
        folders.folders[0].source,
        tcm_core::folders::FolderSource::Auto
    );
    assert!(folders.folders[0].enabled);
    assert_eq!(store.all_items().unwrap()[0].title, "Film");
    fs::rename(&source, source.with_file_name("offline-original")).unwrap();
    drop(store);
    let store = Store::open(&base.join("state.sqlite")).unwrap();
    assert_eq!(store.folder_settings().unwrap().folders[0].name, "电影库");
    assert_eq!(store.all_items().unwrap()[0].title, "Film");
}

#[test]
fn any_new_stack_configuration_choice_prevents_summary_directory_recovery() {
    let (_temp, store, roaming, web, source) = setup();
    let chosen = store
        .configure("new-choice", Configuration::default())
        .unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "generatedAt": "2026-09-12T12:00:00Z",
            "scanStats": {},
            "items": {},
            "libraryRoots": [web]
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = capture(&store, &web, &roaming).unwrap().unwrap();
    assert_eq!(receipt.configuration, chosen);
    assert_eq!(store.configuration().unwrap(), chosen);
    assert!(store.folder_settings().unwrap().folders.is_empty());
}

#[test]
fn matching_original_catalog_restores_the_readonly_list_while_offline_and_forces_future_reparse() {
    let (temp, store, roaming, web, source) = setup();
    let base = temp.path().canonicalize().unwrap();
    let movies = base.join("offline movies");
    let tv = base.join("offline tv");
    let movie_path = movies.join("Film/movie.nfo");
    let episode_path = tv.join("Show/Season 01/episode.nfo");
    let invalid_path = tv.join("Show/broken.nfo");
    fs::create_dir(&movies).unwrap();
    fs::create_dir(&tv).unwrap();
    store
        .configure(
            "restored-roots",
            Configuration {
                roots: vec![
                    LibraryRoot {
                        id: "movie-root".into(),
                        space: Space::Movie,
                        path: movies.to_string_lossy().into(),
                    },
                    LibraryRoot {
                        id: "tv-root".into(),
                        space: Space::Tv,
                        path: tv.to_string_lossy().into(),
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();
    fs::remove_dir(&movies).unwrap();
    fs::remove_dir(&tv).unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 5,
            "generatedAt": "2026-09-12T12:00:00Z",
            "libraryRoots": [movies, tv],
            "indexedTitles": 1,
            "catalogCount": 3,
            "scanStats": {},
            "items": {"tt1234567": {"Runtime": ["100 min"]}}
        }))
        .unwrap(),
    )
    .unwrap();
    let catalog = serde_json::to_vec(&serde_json::json!({
        "generatedAt": "2026-09-12T12:00:01Z",
        "count": 3,
        "items": [
            {
                "path": movie_path,
                "type": "Movie",
                "title": "Film",
                "originalTitle": "Original Film",
                "showTitle": "",
                "seriesTitle": "",
                "year": "2026",
                "season": "",
                "episode": "",
                "imdb": "tt1234567",
                "libraryKind": "movies",
                "hasTechnicalSpecs": true,
                "tagCount": 2,
                "tags": [
                    {"value": "External", "ownership": "external", "engine": ""},
                    {"value": "Generated", "ownership": "generated", "engine": "rules"}
                ],
                "specs": {"Runtime": ["100 min"]},
                "error": ""
            },
            {
                "path": episode_path,
                "type": "Episode",
                "title": "Episode",
                "originalTitle": "",
                "showTitle": "",
                "seriesTitle": "Recovered Show",
                "year": "2026",
                "season": "1",
                "episode": "2",
                "imdb": "",
                "libraryKind": "tv",
                "hasTechnicalSpecs": false,
                "tagCount": 0,
                "tags": [],
                "specs": {},
                "error": ""
            },
            {
                "path": invalid_path,
                "type": "InvalidNFO",
                "title": "broken",
                "originalTitle": "",
                "showTitle": "",
                "seriesTitle": "",
                "year": "",
                "season": "",
                "episode": "",
                "imdb": "",
                "libraryKind": "tv",
                "hasTechnicalSpecs": false,
                "tagCount": 0,
                "tags": [],
                "specs": {},
                "error": "invalid XML"
            }
        ]
    }))
    .unwrap();
    fs::write(source.join("manager-catalog.json"), &catalog).unwrap();
    // The old parser cache and stamps cannot claim the new parser revision.
    // They remain untouched at the source while the display catalog is usable.
    let cache = b"legacy parser cache";
    fs::write(source.join("manager-items-cache.json"), cache).unwrap();
    let catalog_modified = fs::metadata(source.join("manager-catalog.json"))
        .unwrap()
        .modified()
        .unwrap();
    let cache_modified = fs::metadata(source.join("manager-items-cache.json"))
        .unwrap()
        .modified()
        .unwrap();
    let receipt = capture(&store, &web, &roaming).unwrap().unwrap();
    assert_eq!(receipt.phase, "imported-with-readonly-catalog");
    assert_eq!(receipt.imported_files, 3);
    assert_eq!(
        store
            .legacy_artifact(&receipt.id, "manager-catalog.json")
            .unwrap(),
        catalog
    );
    assert!(store
        .legacy_artifact(&receipt.id, "manager-items-cache.json")
        .is_err());
    assert_eq!(
        fs::read(source.join("manager-catalog.json")).unwrap(),
        catalog
    );
    assert_eq!(
        fs::metadata(source.join("manager-catalog.json"))
            .unwrap()
            .modified()
            .unwrap(),
        catalog_modified
    );
    assert_eq!(
        fs::read(source.join("manager-items-cache.json")).unwrap(),
        cache
    );
    assert_eq!(
        fs::metadata(source.join("manager-items-cache.json"))
            .unwrap()
            .modified()
            .unwrap(),
        cache_modified
    );
    let items = store.all_items().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items
        .iter()
        .all(|item| item.parser_revision == 0 && item.source_hash.is_empty()));
    let movie = items.iter().find(|item| item.kind == "Movie").unwrap();
    assert_eq!(movie.root_id, "movie-root");
    assert_eq!(movie.tags[1].ownership, Ownership::Generated);
    let episode = items.iter().find(|item| item.kind == "Episode").unwrap();
    assert_eq!(episode.root_id, "tv-root");
    assert_eq!(episode.show_title, "Recovered Show");
    let invalid = items.iter().find(|item| item.kind == "InvalidNFO").unwrap();
    assert_eq!(invalid.space, Space::Tv);
    assert_eq!(invalid.error.as_ref().unwrap().message, "invalid XML");
    let summary = store.catalog_summary().unwrap();
    assert_eq!(
        summary.generated_at.as_deref(),
        Some("2026-09-12T12:00:01Z")
    );
    assert_eq!((summary.movie, summary.tv, summary.errors), (1, 2, 1));

    fs::create_dir_all(movie_path.parent().unwrap()).unwrap();
    let nfo = b"\xef\xbb\xbf<movie>\r\n<title>Live NFO</title>\r\n<uniqueid type=\"imdb\">tt1234567</uniqueid>\r\n<technicalspecs source=\"IMDb\"><section name=\"Runtime\"><item>121 min</item></section></technicalspecs>\r\n</movie>\r\n";
    fs::write(&movie_path, nfo).unwrap();
    let nfo_modified = fs::metadata(&movie_path).unwrap().modified().unwrap();
    store
        .submit(ScanRequest {
            operation_id: "returned-root-scan".into(),
            space: Space::Movie,
            root_ids: vec!["movie-root".into()],
        })
        .unwrap();
    assert_eq!(
        store.run_next(|| false, |_| {}).unwrap().unwrap().state,
        TaskState::Completed
    );
    let reparsed = store
        .all_items()
        .unwrap()
        .into_iter()
        .find(|item| item.path == movie_path.to_string_lossy())
        .unwrap();
    assert_eq!(reparsed.title, "Live NFO");
    assert_eq!(reparsed.specs["Runtime"], ["121 min"]);
    assert_eq!(reparsed.parser_revision, tcm_core::library::PARSER_REVISION);
    assert!(!reparsed.source_hash.is_empty());
    assert_eq!(fs::read(&movie_path).unwrap(), nfo);
    assert_eq!(
        fs::metadata(&movie_path).unwrap().modified().unwrap(),
        nfo_modified
    );

    fs::rename(&source, source.with_file_name("offline-original")).unwrap();
    drop(store);
    let store = Store::open(&base.join("state.sqlite")).unwrap();
    assert_eq!(store.all_items().unwrap().len(), 3);
}

#[test]
fn mismatched_catalog_rolls_back_and_can_retry_after_the_source_is_consistent() {
    let (temp, store, roaming, web, source) = setup();
    let base = temp.path().canonicalize().unwrap();
    let movies = base.join("movies");
    fs::create_dir(&movies).unwrap();
    store
        .configure(
            "roots",
            Configuration {
                roots: vec![LibraryRoot {
                    id: "movie-root".into(),
                    space: Space::Movie,
                    path: movies.to_string_lossy().into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    fs::remove_dir(&movies).unwrap();
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 5,
            "generatedAt": "2026-09-12T12:00:00Z",
            "libraryRoots": [movies],
            "catalogCount": 1,
            "items": {}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("manager-catalog.json"),
        br#"{"generatedAt":"2026-09-12T12:00:01Z","count":0,"items":[]}"#,
    )
    .unwrap();
    assert_eq!(
        capture(&store, &web, &roaming).unwrap_err().code,
        "migration-catalog-invalid"
    );
    let db = rusqlite::Connection::open(base.join("state.sqlite")).unwrap();
    for table in ["legacy_artifacts", "items"] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM operations WHERE id LIKE 'emby-diagnostics-%'",
            [],
            |row| row.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    fs::write(
        source.join("manager-index-summary.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 5,
            "generatedAt": "2026-09-12T12:00:00Z",
            "libraryRoots": [movies],
            "catalogCount": 0,
            "items": {}
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        capture(&store, &web, &roaming).unwrap().unwrap().phase,
        "imported-with-readonly-catalog"
    );
}
#[test]
fn credentials_and_symbolic_sources_never_enter_the_diagnostic_archive() {
    for secret in [
        br#"{"api_key":"private"}"#.as_slice(),
        br#"{"api\u005fkey":"private""#,
    ] {
        let (_temp, store, roaming, web, source) = setup();
        fs::write(source.join("manager-xml-errors.json"), secret).unwrap();
        assert_eq!(
            capture(&store, &web, &roaming).unwrap_err().code,
            "migration-credential-boundary"
        );
        assert!(store.index_summary().unwrap().is_none());
    }
    #[cfg(unix)]
    {
        let (_temp, store, roaming, web, source) = setup();
        let old = source.with_file_name("real-original");
        fs::rename(&source, &old).unwrap();
        std::os::unix::fs::symlink(&old, &source).unwrap();
        assert_eq!(
            capture(&store, &web, &roaming).unwrap_err().code,
            "ambiguous-path"
        );
        assert!(store.index_summary().unwrap().is_none());
    }
}

#[test]
fn an_existing_reviewed_archive_is_reused_instead_of_copying_the_same_files_again() {
    let (temp, store, roaming, web, source) = setup();
    let plan = store
        .prepare_migration("reviewed", &source, "tcm-state")
        .unwrap();
    store
        .apply_migration("reviewed", &plan.fingerprint)
        .unwrap();
    let config = store.configuration().unwrap();
    fs::rename(&source, source.with_file_name("offline")).unwrap();
    assert_eq!(
        capture(&store, &web, &roaming).unwrap().unwrap().id,
        "reviewed"
    );
    let db = rusqlite::Connection::open(temp.path().join("state.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM legacy_artifacts", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(store.configuration().unwrap(), config);
}
