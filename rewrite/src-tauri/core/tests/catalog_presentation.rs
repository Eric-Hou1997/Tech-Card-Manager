use tcm_core::{store::Store, ui::*, *};
fn item(id: &str, kind: &str, imdb: &str, valid_specs: bool) -> MediaItem {
    MediaItem {
        parser_revision: 1,
        id: id.into(),
        root_id: "root".into(),
        space: if kind == "Movie" {
            Space::Movie
        } else {
            Space::Tv
        },
        path: format!("/media/{id}.nfo"),
        source_hash: String::new(),
        title: id.into(),
        original_title: String::new(),
        show_title: String::new(),
        year: "2026".into(),
        imdb: imdb.into(),
        kind: kind.into(),
        season: String::new(),
        episode: String::new(),
        specs: if valid_specs {
            [("Camera".into(), vec!["ARRI Alexa".into()])].into()
        } else {
            Default::default()
        },
        tags: vec![],
        error: None,
    }
}
#[test]
fn console_uses_the_actual_deduplicated_card_index_and_retains_all_nfo_counts() {
    let mut items = vec![
        item("movie", "Movie", "tt0061452", true),
        item("duplicate", "Movie", "tt0061452", true),
        item("show", "Series", "tt0944947", true),
        item("episode", "Episode", "tt1480055", true),
        item("no-imdb", "Movie", "", true),
        item("no-specs", "Movie", "tt0381061", false),
        item("broken", "Movie", "tt1663662", true),
    ];
    items[6].error = Some(AppError::new("xml-invalid", "Malformed XML"));
    let counts = summary(&items);
    assert_eq!(counts.total, 7);
    assert_eq!(counts.movie, 5);
    assert_eq!(counts.tv, 2);
    assert_eq!(counts.errors, 1);
    assert_eq!(counts.displayable, 2);
    assert_eq!(
        counts.displayable as usize,
        tcm_core::emby::public_index(&items, "now".into())
            .items
            .len()
    );
}
#[test]
fn read_only_status_filters_preserve_error_precedence_and_restore_per_space() {
    let valid = item("valid", "Movie", "tt0061452", true);
    let missing = item("missing", "Movie", "tt0061452", false);
    let mut broken = valid.clone();
    broken.error = Some(AppError::new("xml-invalid", "Malformed XML"));
    for (filter, expected) in [
        (SpecFilter::All, [true, true, true]),
        (SpecFilter::Ready, [true, false, false]),
        (SpecFilter::Missing, [false, true, false]),
        (SpecFilter::Error, [false, false, true]),
    ] {
        let view = LibraryView {
            spec_filter: Some(filter),
            ..Default::default()
        };
        for (item, expected) in [&valid, &missing, &broken].into_iter().zip(expected) {
            assert_eq!(matches(item, &Space::Movie, &view), expected);
            assert!(!matches(item, &Space::Tv, &view));
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.sqlite");
    let store = Store::open(&path).unwrap();
    let mut state = store.ui_state().unwrap();
    state.movie.spec_filter = Some(SpecFilter::Ready);
    state.tv.spec_filter = Some(SpecFilter::Missing);
    store.save_ui_state("filters", state).unwrap();
    drop(store);
    let restored = Store::open(&path).unwrap().ui_state().unwrap();
    assert_eq!(restored.movie.spec_filter, Some(SpecFilter::Ready));
    assert_eq!(restored.tv.spec_filter, Some(SpecFilter::Missing));
}

#[test]
fn original_manager_titles_are_parsed_and_resolve_within_the_indexed_root() {
    use std::path::Path;
    let root = LibraryRoot {
        id: "tv".into(),
        space: Space::Tv,
        path: "/media".into(),
    };
    let parse = |path: &str, xml: &str| {
        tcm_core::library::parse(&root, Path::new(path), xml.as_bytes()).unwrap()
    };
    let show = parse(
        "/media/show/tvshow.nfo",
        "<tvshow><title>节目 &amp; 一</title><originaltitle>Original Show</originaltitle></tvshow>",
    );
    let episode = parse("/media/show/Season 1/e.nfo", "<episodedetails><title>第一集</title><originaltitle>Original Episode</originaltitle><season>1</season><episode>2</episode></episodedetails>");
    let explicit = parse(
        "/media/show/explicit.nfo",
        "<episodedetails><showtitle>显式节目</showtitle></episodedetails>",
    );
    let mut foreign = episode.clone();
    foreign.id = "foreign".into();
    foreign.root_id = "another-root".into();
    let mut malformed = episode.clone();
    malformed.id = "malformed".into();
    malformed.path = "/media/orphan/e.nfo".into();
    let mut wrong_name = show.clone();
    wrong_name.id = "wrong-name".into();
    wrong_name.path = "/media/orphan/show.nfo".into();
    let rows = manager_catalog(vec![
        show, episode, explicit, foreign, malformed, wrong_name,
    ]);
    let row = |id: &str| rows.iter().find(|row| row.item.id == id).unwrap();
    assert_eq!(row("foreign").series_title, "");
    assert_eq!(row("malformed").series_title, "");
    let episode = rows
        .iter()
        .find(|row| {
            row.item.original_title == "Original Episode"
                && row.item.root_id == "tv"
                && row.item.id != "malformed"
        })
        .unwrap();
    assert_eq!(episode.series_title, "节目 & 一");
    assert!(rows
        .iter()
        .any(|row| row.item.show_title == "显式节目" && row.series_title == "显式节目"));
}

#[test]
fn catalog_title_fields_survive_scan_restart_without_changing_nfo_bytes_or_time() {
    use std::fs;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let media = dir.join("media");
    fs::create_dir(&media).unwrap();
    let path = media.join("movie.nfo");
    let bytes="\u{feff}<movie>\r\n<originaltitle>Original Film</originaltitle><title>电影</title></movie>";
    fs::write(&path, bytes).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let db = dir.join("workspace.sqlite");
    let store = Store::open(&db).unwrap();
    store
        .configure(
            "configure",
            Configuration {
                roots: vec![LibraryRoot {
                    id: "movie".into(),
                    space: Space::Movie,
                    path: media.to_string_lossy().into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    store
        .submit(ScanRequest {
            operation_id: "scan".into(),
            space: Space::Movie,
            root_ids: vec!["movie".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    let rows = manager_catalog(store.all_items().unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].item.original_title, "Original Film");
    drop(store);
    // A v2 cache entry with identical source bytes must be reparsed, not reused
    // without the original-title fields needed by the existing Manager search.
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute("UPDATE items SET body=json_remove(json_set(body,'$.parser_revision',2),'$.original_title','$.show_title')", []).unwrap();
    drop(connection);
    let store = Store::open(&db).unwrap();
    assert!(store.all_items().unwrap()[0].original_title.is_empty());
    store
        .submit(ScanRequest {
            operation_id: "rescan-old-cache".into(),
            space: Space::Movie,
            root_ids: vec!["movie".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(
        store.all_items().unwrap()[0].original_title,
        "Original Film"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn console_eligible_counts_keep_duplicate_titles_and_exclude_both_seasons_and_episodes() {
    let items = vec![
        item("one", "Movie", "tt0061452", true),
        item("two", "Movie", "tt0061452", true),
        item("show", "Series", "tt0944947", true),
        item("season", "Season", "tt0944947", true),
        item("episode", "Episode", "tt1480055", true),
        item("bad-id", "Episode", "invalid", true),
        item("missing", "Movie", "tt0381061", false),
    ];
    let counts = summary(&items);
    assert_eq!(counts.web_eligible, 3);
    assert_eq!(counts.episodes_excluded, 2);
    assert_eq!(counts.displayable, 2);
    assert_eq!(counts.total, 7);
}

#[test]
fn locale_changes_and_rejected_all_disabled_rows_do_not_claim_configured_roots() {
    use tcm_core::folders::*;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let store = Store::open(&root.join("state.sqlite")).unwrap();
    assert!(!store.catalog_summary().unwrap().roots_configured);
    let config = store
        .configure(
            "locale-only",
            Configuration {
                revision: 0,
                locale: Locale::English,
                roots: vec![],
            },
        )
        .unwrap();
    assert!(!store.catalog_summary().unwrap().roots_configured);
    let settings = FolderSettings {
        revision: config.revision,
        folders: vec![MediaFolder {
            id: "disabled".into(),
            path: root.join("offline").to_string_lossy().into_owned(),
            name: "Offline".into(),
            kind: FolderKind::Movies,
            source: FolderSource::Manual,
            enabled: false,
        }],
    };
    assert_eq!(
        store
            .save_folder_settings("disabled-folder", settings)
            .unwrap_err()
            .code,
        "invalid-folders"
    );
    assert!(!store.catalog_summary().unwrap().roots_configured);
}
