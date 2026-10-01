use std::{fs, path::Path};
use tcm_core::{folders::*, store::Store, *};
fn setup() -> (tempfile::TempDir, Store, FolderSettings) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    fs::write(
        media.join("movie.nfo"),
        b"<movie><title>Movie</title></movie>",
    )
    .unwrap();
    fs::write(
        media.join("tvshow.nfo"),
        b"<tvshow><title>Series</title></tvshow>",
    )
    .unwrap();
    let store = Store::open(&root.join("state.sqlite")).unwrap();
    let settings = FolderSettings {
        revision: 0,
        folders: vec![MediaFolder {
            id: "mixed-folder".into(),
            path: media.to_str().unwrap().into(),
            name: "Mixed library".into(),
            kind: FolderKind::Auto,
            source: FolderSource::Manual,
            enabled: true,
        }],
    };
    (temp, store, settings)
}
fn scan(store: &Store, config: &Configuration, id: &str, space: Space) {
    store
        .submit(ScanRequest {
            operation_id: id.into(),
            space: space.clone(),
            root_ids: config
                .roots
                .iter()
                .filter(|r| r.space == space)
                .map(|r| r.id.clone())
                .collect(),
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
}
#[test]
fn original_auto_folder_indexes_both_spaces_without_mutating_nfos() {
    let (_temp, store, settings) = setup();
    let media = Path::new(&settings.folders[0].path).to_owned();
    let before: Vec<_> = ["movie.nfo", "tvshow.nfo"]
        .iter()
        .map(|name| {
            (
                fs::read(media.join(name)).unwrap(),
                fs::metadata(media.join(name)).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let saved = store.save_folder_settings("save", settings).unwrap();
    assert_eq!(saved.configuration.roots.len(), 2);
    scan(&store, &saved.configuration, "scan-movie", Space::Movie);
    scan(&store, &saved.configuration, "scan-tv", Space::Tv);
    let summary = tcm_core::ui::summary(&store.all_items().unwrap());
    assert_eq!((summary.movie, summary.tv), (1, 1));
    for (i, name) in ["movie.nfo", "tvshow.nfo"].iter().enumerate() {
        assert_eq!(fs::read(media.join(name)).unwrap(), before[i].0);
        assert_eq!(
            fs::metadata(media.join(name)).unwrap().modified().unwrap(),
            before[i].1
        );
    }
}
#[test]
fn disabled_folder_survives_restart_and_cannot_be_scanned_until_reenabled() {
    let (temp, store, settings) = setup();
    let saved = store.save_folder_settings("save", settings).unwrap();
    scan(&store, &saved.configuration, "scan", Space::Movie);
    let keep = Path::new(&saved.settings.folders[0].path)
        .parent()
        .unwrap()
        .join("keep");
    fs::create_dir(&keep).unwrap();
    let mut next = saved.settings;
    next.folders[0].enabled = false;
    next.folders.push(MediaFolder {
        id: "keep".into(),
        path: keep.to_str().unwrap().into(),
        name: "Keep".into(),
        kind: FolderKind::Movies,
        source: FolderSource::Manual,
        enabled: true,
    });
    let receipt = store.save_folder_settings("disable", next.clone()).unwrap();
    assert_eq!(receipt.configuration.roots.len(), 1);
    assert_eq!(tcm_core::ui::summary(&store.all_items().unwrap()).total, 0);
    assert_eq!(
        store.save_folder_settings("disable", next).unwrap(),
        receipt
    );
    let old_root = &saved.configuration.roots[0];
    assert!(store
        .submit(ScanRequest {
            operation_id: "invalid-scan".into(),
            space: old_root.space.clone(),
            root_ids: vec![old_root.id.clone()]
        })
        .is_err());
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    let mut restored = store.folder_settings().unwrap();
    assert_eq!(restored.folders.len(), 2);
    assert!(!restored.folders[0].enabled);
    restored.folders[0].enabled = true;
    let enabled = store.save_folder_settings("enable", restored).unwrap();
    assert_eq!(enabled.configuration.roots.len(), 3);
}
#[test]
fn conflicts_and_active_work_never_partially_save_folder_rows() {
    let (_temp, store, settings) = setup();
    let saved = store
        .save_folder_settings("save", settings.clone())
        .unwrap();
    assert_eq!(
        store
            .save_folder_settings("stale", settings)
            .unwrap_err()
            .code,
        "configuration-conflict"
    );
    let mut draft = saved.settings.clone();
    draft.folders[0].name = "Renamed".into();
    draft.folders[0].kind = FolderKind::Movies;
    store
        .submit(ScanRequest {
            operation_id: "active".into(),
            space: Space::Tv,
            root_ids: vec![saved.configuration.roots[1].id.clone()],
        })
        .unwrap();
    assert_eq!(
        store
            .save_folder_settings("change", draft)
            .unwrap_err()
            .code,
        "active-task"
    );
    assert_eq!(store.folder_settings().unwrap(), saved.settings);
    assert_eq!(store.configuration().unwrap(), saved.configuration);
}
#[test]
fn original_configuration_updates_reconcile_rows_without_losing_disabled_entries() {
    let (_temp, store, settings) = setup();
    let saved = store.save_folder_settings("save", settings).unwrap();
    let mut config = saved.configuration;
    config.locale = Locale::English;
    let config = store.configure("locale", config).unwrap();
    let rows = store.folder_settings().unwrap();
    assert_eq!(rows.revision, config.revision);
    assert_eq!(rows.folders, saved.settings.folders);
    let mut invalid = rows;
    invalid.folders[0].enabled = false;
    assert_eq!(
        store
            .save_folder_settings("empty", invalid)
            .unwrap_err()
            .code,
        "invalid-folders"
    );
}
#[test]
fn invalid_duplicate_or_overlapping_candidates_leave_saved_state_unchanged() {
    let (_temp, store, settings) = setup();
    let saved = store.save_folder_settings("save", settings).unwrap();
    for (case, path) in [
        ("duplicate", saved.settings.folders[0].path.clone()),
        ("relative", "relative/path".into()),
    ] {
        let mut draft = saved.settings.clone();
        let mut extra = draft.folders[0].clone();
        extra.id = case.into();
        extra.path = path;
        draft.folders.push(extra);
        assert!(store.save_folder_settings(case, draft).is_err());
        assert_eq!(store.folder_settings().unwrap(), saved.settings);
    }
    let mut changed = saved.settings.clone();
    changed.folders[0].name = "Other".into();
    assert_eq!(
        store
            .save_folder_settings("save", changed)
            .unwrap_err()
            .code,
        "operation-conflict"
    );
}
#[test]
fn offline_existing_folders_and_replayed_receipts_preserve_original_configuration() {
    let (_temp, store, settings) = setup();
    let request = settings.clone();
    let saved = store.save_folder_settings("save", settings).unwrap();
    let media = Path::new(&saved.settings.folders[0].path);
    fs::rename(media, media.with_extension("offline")).unwrap();
    assert_eq!(store.save_folder_settings("save", request).unwrap(), saved);
    let mut changed = saved.settings.clone();
    changed.folders[0].name = "Offline folder".into();
    let next = store.save_folder_settings("rename", changed).unwrap();
    assert_eq!(next.configuration.roots, saved.configuration.roots);
}
#[cfg(unix)]
#[test]
fn replacing_an_existing_folder_with_a_symlink_is_rejected() {
    let (temp, store, settings) = setup();
    let saved = store.save_folder_settings("save", settings).unwrap();
    let media = Path::new(&saved.settings.folders[0].path);
    let moved = temp.path().join("moved");
    fs::rename(media, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, media).unwrap();
    let mut changed = saved.settings.clone();
    changed.folders[0].name = "Symlink".into();
    assert!(store.save_folder_settings("symlink", changed).is_err());
    assert_eq!(store.folder_settings().unwrap(), saved.settings);
}

#[test]
fn first_offline_selection_survives_restart_and_indexes_when_it_returns() {
    let (temp, store, mut settings) = setup();
    let offline = temp.path().canonicalize().unwrap().join("offline/nas/电影");
    settings.folders[0].path = offline.to_string_lossy().into();
    settings.folders[0].kind = FolderKind::Movies;
    settings.folders[0].source = FolderSource::Auto;
    let saved = store
        .save_folder_settings("offline-first", settings)
        .unwrap();
    assert!(!offline.exists());
    scan(&store, &saved.configuration, "offline-check", Space::Movie);
    assert_eq!(
        store.task("offline-check").unwrap().state,
        TaskState::Failed
    );
    assert_eq!(store.configuration().unwrap(), saved.configuration);
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(store.folder_settings().unwrap(), saved.settings);
    fs::create_dir_all(&offline).unwrap();
    let nfo = offline.join("movie.nfo");
    let bytes = b"\xef\xbb\xbf<movie>\r\n<title>Returned</title>\r\n</movie>\r\n";
    fs::write(&nfo, bytes).unwrap();
    let modified = fs::metadata(&nfo).unwrap().modified().unwrap();
    scan(&store, &saved.configuration, "online-check", Space::Movie);
    assert_eq!(
        store.task("online-check").unwrap().state,
        TaskState::Completed
    );
    let items = store.all_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Returned");
    assert_eq!(fs::read(&nfo).unwrap(), bytes);
    assert_eq!(fs::metadata(&nfo).unwrap().modified().unwrap(), modified);
    // A later outage retains the previously read title instead of deleting it.
    fs::rename(&offline, offline.with_extension("disconnected")).unwrap();
    scan(&store, &saved.configuration, "offline-again", Space::Movie);
    assert!(store
        .all_items()
        .unwrap()
        .iter()
        .any(|item| item.title == "Returned"));
}

#[test]
fn offline_paths_still_reject_parent_traversal_files_disk_roots_and_excess_rows() {
    let (temp, store, settings) = setup();
    let root = temp.path().canonicalize().unwrap();
    for (id, path) in [
        ("parent", root.join("missing/../other")),
        ("file", root.join("media/movie.nfo/child")),
        ("disk", root.ancestors().last().unwrap().to_path_buf()),
    ] {
        let mut draft = settings.clone();
        draft.folders[0].path = path.to_string_lossy().into();
        assert!(store.save_folder_settings(id, draft).is_err(), "{id}");
        assert!(store.configuration().unwrap().roots.is_empty());
    }
    let mut excess = settings.clone();
    excess.folders = (0..257)
        .map(|id| MediaFolder {
            id: format!("folder-{id}"),
            path: root.join(format!("offline-{id}")).to_string_lossy().into(),
            ..settings.folders[0].clone()
        })
        .collect();
    assert_eq!(
        store
            .save_folder_settings("too-many", excess)
            .unwrap_err()
            .message,
        "媒体目录不能超过 256 个"
    );
}

#[cfg(unix)]
#[test]
fn offline_selection_does_not_authorize_a_symlink_when_the_directory_returns() {
    let (temp, store, mut settings) = setup();
    let root = temp.path().canonicalize().unwrap();
    let offline = root.join("offline");
    settings.folders[0].path = offline.join("movies").to_string_lossy().into();
    settings.folders[0].kind = FolderKind::Movies;
    let saved = store.save_folder_settings("offline", settings).unwrap();
    let outside = root.join("outside");
    fs::create_dir_all(outside.join("movies")).unwrap();
    fs::write(
        outside.join("movies/movie.nfo"),
        b"<movie><title>Outside</title></movie>",
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, &offline).unwrap();
    scan(&store, &saved.configuration, "unsafe-return", Space::Movie);
    assert_eq!(
        store.task("unsafe-return").unwrap().state,
        TaskState::Failed
    );
    assert!(!store
        .all_items()
        .unwrap()
        .iter()
        .any(|item| item.title == "Outside"));
    let mut rename = saved.settings.clone();
    rename.folders[0].name = "New name".into();
    assert_eq!(
        store
            .save_folder_settings("unsafe-save", rename)
            .unwrap_err()
            .code,
        "ambiguous-path"
    );
    assert_eq!(store.folder_settings().unwrap(), saved.settings);
}
