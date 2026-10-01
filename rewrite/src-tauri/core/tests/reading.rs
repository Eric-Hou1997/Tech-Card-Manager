use std::{fs, path::Path};
use tcm_core::{store::Store, *};
fn fixture() -> (&'static str, &'static str) {
    ("电影.nfo","\u{feff}<movie>\r\n<title>电影 &amp; 测试</title><year>1967</year><uniqueid type=\"imdb\"> </uniqueid><uniqueid type=\" imdb \" >tt0064757</uniqueid><tag>External</tag><tag>1.43 : 1 (scene)</tag><tag>Manual</tag><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item><item>ARRI</item></section><manualtags owner=\" IMDb Tech Manager \" engine=\"must-be-ignored\"><tag>Manual</tag></manualtags><generatedtags owner=\" IMDb Tech Manager \" engine=\" LOCAL \" ><tag>1.43:1</tag></generatedtags></technicalspecs></movie>")
}
fn setup() -> (tempfile::TempDir, Store, Configuration) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    fs::create_dir(dir.join("movies")).unwrap();
    fs::create_dir(dir.join("tv")).unwrap();
    let store = Store::open(&dir.join("state.sqlite")).unwrap();
    let config = store
        .configure(
            "setup",
            Configuration {
                revision: 0,
                locale: Locale::English,
                roots: vec![
                    LibraryRoot {
                        id: "movie".into(),
                        space: Space::Movie,
                        path: dir.join("movies").to_str().unwrap().into(),
                    },
                    LibraryRoot {
                        id: "tv".into(),
                        space: Space::Tv,
                        path: dir.join("tv").to_str().unwrap().into(),
                    },
                ],
            },
        )
        .unwrap();
    (temp, store, config)
}
fn request(id: &str) -> ScanRequest {
    ScanRequest {
        operation_id: id.into(),
        space: Space::Movie,
        root_ids: vec!["movie".into()],
    }
}
fn query(space: Space) -> CatalogQuery {
    CatalogQuery {
        space,
        search: String::new(),
        only_errors: false,
        offset: 0,
        limit: 100,
    }
}
#[test]
fn readonly_nfo_identity_scope_and_ownership() {
    let (_temp, store, config) = setup();
    let root = Path::new(&config.roots[0].path);
    let (name, raw) = fixture();
    let path = root.join(name);
    fs::write(&path, raw).unwrap();
    fs::write(root.join("duplicate.nfo"), raw).unwrap();
    fs::write(Path::new(&config.roots[1].path).join("tv.nfo"), raw).unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let task = store.submit(request("scan1")).unwrap();
    assert_eq!(task.locale, Locale::English);
    store.run_next(|| false, |_| {}).unwrap();
    let page = store.query(query(Space::Movie)).unwrap();
    assert_eq!(page.total, 2);
    assert_ne!(page.items[0].id, page.items[1].id);
    assert_eq!(page.items[0].imdb, "tt0064757");
    assert_eq!(page.items[0].tags[0].ownership, Ownership::External);
    assert_eq!(page.items[0].tags[1].ownership, Ownership::Generated);
    assert_eq!(page.items[0].tags[1].engine, "local");
    assert_eq!(page.items[0].tags[2].ownership, Ownership::Manual);
    assert_eq!(page.items[0].tags[2].engine, "");
    assert_eq!(page.items[0].specs["Camera"], vec!["ARRI"]);
    assert_eq!(page.items[0].title, "电影 & 测试");
    assert_eq!(store.query(query(Space::Tv)).unwrap().total, 0);
    assert_eq!(fs::read(&path).unwrap(), raw.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}
#[test]
fn unicode_xml_files_keep_original_bytes_and_are_indexed_without_utf8_conversion() {
    let (_temp, store, config) = setup();
    let root = Path::new(&config.roots[0].path);
    let title = "电影 Café 🎬";
    let mut originals = Vec::new();
    for width in [16, 32] {
        for little in [true, false] {
            for bom in [true, false] {
                let source = format!("<?xml version=\"1.0\" encoding=\"UTF-{width}\"?><movie><title>{title}</title><uniqueid type=\"imdb\">tt0061452</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item></section></technicalspecs></movie>");
                let mut bytes = Vec::new();
                if width == 16 {
                    for word in bom
                        .then_some(0xfeff)
                        .into_iter()
                        .chain(source.encode_utf16())
                    {
                        bytes.extend(if little {
                            word.to_le_bytes()
                        } else {
                            word.to_be_bytes()
                        });
                    }
                } else {
                    for word in bom
                        .then_some(0xfeff)
                        .into_iter()
                        .chain(source.chars().map(u32::from))
                    {
                        bytes.extend(if little {
                            word.to_le_bytes()
                        } else {
                            word.to_be_bytes()
                        });
                    }
                }
                let path = root.join(format!("utf{width}-{little}-{bom}.nfo"));
                fs::write(&path, &bytes).unwrap();
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                originals.push((path, bytes, modified));
            }
        }
    }
    store.submit(request("unicode-xml")).unwrap();
    let task = store.run_next(|| false, |_| {}).unwrap().unwrap();
    assert_eq!(task.errors, 0);
    let page = store.query(query(Space::Movie)).unwrap();
    assert_eq!(page.total, 8);
    for item in page.items {
        assert_eq!(item.title, title);
        assert_eq!(item.imdb, "tt0061452");
        assert_eq!(item.specs["Camera"], vec!["ARRI"]);
        let (_, bytes, _) = originals
            .iter()
            .find(|(path, _, _)| path.to_str() == Some(&item.path))
            .unwrap();
        assert_eq!(item.source_hash, hash(bytes));
    }
    for (path, bytes, modified) in originals {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
}
#[test]
fn malformed_unicode_xml_is_rejected_without_replacement_or_file_changes() {
    let (_temp, _store, config) = setup();
    let root = &config.roots[0];
    let path = Path::new(&root.path).join("malformed-unicode.nfo");
    for raw in [
        vec![0xff, 0xfe, 0x3c],
        vec![0xff, 0xfe, 0x00, 0xd8, 0, 0],
        vec![0xff, 0xfe, 0, 0, 0xff, 0xff, 0x11, 0],
        vec![0, 0, 0xfe, 0xff, 0, 0, 0xd8, 0],
    ] {
        fs::write(&path, &raw).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let error = library::read(root, &path).unwrap_err();
        assert_eq!(error.code, "invalid-encoding");
        assert_eq!(error.path.as_deref(), path.to_str());
        assert_eq!(fs::read(&path).unwrap(), raw);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
    fs::write(&path, fixture().1).unwrap();
    assert_eq!(library::read(root, &path).unwrap().imdb, "tt0064757");
    for declared in ["UTF-8", "UTF-16BE", "not-a-real-encoding"] {
        let source = format!("<?xml version=\"1.0\" encoding=\"{declared}\"?><movie/>");
        let bytes = std::iter::once(0xfeff)
            .chain(source.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            library::read(root, &path).unwrap_err().code,
            "invalid-encoding"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
#[test]
fn html_entity_tag_comparison_retains_original_manifest_ownership() {
    let (_temp, _store, config) = setup();
    let path = Path::new(&config.roots[0].path).join("html-ownership.nfo");
    let pairs = [
        ("A&amp;B", "A&B"),
        ("1.43&nbsp; : 1 (scene)", "1.43:1"),
        ("Caf&eacute;", "Café"),
        ("&#x1F3AC;", "🎬"),
        ("&# +65;", "A"),
        ("&amp;amp;", "&amp;"),
        ("&apos;", "'"),
        ("&NoBreak;", "\u{2060}"),
    ];
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let tags = pairs
        .iter()
        .map(|(value, _)| format!("<tag>{}</tag>", escape(value)))
        .collect::<String>();
    let owned = pairs
        .iter()
        .map(|(_, value)| format!("<tag>{}</tag>", escape(value)))
        .collect::<String>();
    let source = format!("<movie><title>Entity ownership</title>{tags}<technicalspecs source=\"IMDb\"><generatedtags owner=\"IMDb Tech Manager\" engine=\"local\">{owned}</generatedtags></technicalspecs></movie>");
    fs::write(&path, &source).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let item = library::read(&config.roots[0], &path).unwrap();
    for (i, tag) in item.tags.iter().enumerate() {
        assert_eq!(tag.value, pairs[i].0);
        // HTML 5-only names remain literal in the original .NET comparator.
        assert_eq!(
            tag.ownership,
            if i == 5 || i == pairs.len() - 1 {
                Ownership::External
            } else {
                Ownership::Generated
            }
        );
    }
    assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    for value in ["&unknown;", "&#xD800;", "&#1114112;", "&NoBreak;", "&AMP;"] {
        assert_eq!(library::canonical_tag(value), value.to_lowercase());
    }
    assert_eq!(library::canonical_tag("&amp;amp;"), "&amp;");
    assert_eq!(library::canonical_tag("&amp;"), "&");
}
#[test]
fn xml_declaration_encodings_are_read_without_rewriting_legacy_nfo_bytes() {
    let (_temp, _store, config) = setup();
    let root = &config.roots[0];
    for (encoding, title, expected) in [
        ("GB2312", b"\xb5\xe7\xd3\xb0".as_slice(), "电影"),
        ("windows-1252", b"Caf\xe9 \x80".as_slice(), "Café €"),
        ("ISO-8859-1", b"Caf\xe9 A\x85B".as_slice(), "Café A\u{85}B"),
        ("Shift_JIS", b"\x93\xfa\x96\x7b".as_slice(), "日本"),
    ] {
        let mut raw =
            format!("<?xml\tversion=\"1.0\" encoding='{encoding}'?><movie><title>").into_bytes();
        raw.extend_from_slice(title);
        raw.extend_from_slice(b"</title><uniqueid type=\"imdb\">tt0061452</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item></section></technicalspecs></movie>");
        let path = Path::new(&root.path).join(format!("{encoding}.nfo"));
        fs::write(&path, &raw).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let item = library::read(root, &path).unwrap();
        assert_eq!(item.title, expected);
        assert_eq!(item.source_hash, hash(&raw));
        assert_eq!(item.imdb, "tt0061452");
        assert_eq!(item.specs["Camera"], vec!["ARRI"]);
        assert_eq!(fs::read(&path).unwrap(), raw);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
    let path = Path::new(&root.path).join("unknown-encoding.nfo");
    let raw = b"<?xml version=\"1.0\" encoding=\"not-a-real-encoding\"?><movie/>";
    fs::write(&path, raw).unwrap();
    assert_eq!(
        library::read(root, &path).unwrap_err().code,
        "invalid-encoding"
    );
    assert_eq!(fs::read(&path).unwrap(), raw);
}
#[test]
fn valid_unrelated_nfo_is_ignored_and_removes_an_older_cached_item() {
    let (temp, store, config) = setup();
    let path = Path::new(&config.roots[0].path).join("changed.nfo");
    fs::write(&path, fixture().1).unwrap();
    store.submit(request("supported")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 1);

    let unrelated = b"<musicvideo><title>Unrelated</title></musicvideo>";
    fs::write(&path, unrelated).unwrap();
    store.submit(request("unrelated")).unwrap();
    let task = store.run_next(|| false, |_| {}).unwrap().unwrap();
    assert_eq!(task.state, TaskState::Completed);
    assert_eq!(task.errors, 0);
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 0);
    let stats = store.index_summary().unwrap().unwrap().scan_stats.unwrap();
    assert_eq!(stats.nfo_seen, 1);
    assert_eq!(stats.nfo_reparsed, 1);
    assert_eq!(stats.xml_read_errors, 0);
    assert_eq!(fs::read(&path).unwrap(), unrelated);

    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    store.submit(request("unchanged-unrelated")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let stats = store.index_summary().unwrap().unwrap().scan_stats.unwrap();
    assert_eq!(stats.nfo_seen, 1);
    assert_eq!(stats.nfo_reparsed, 0);
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 0);

    fs::remove_file(&path).unwrap();
    store.submit(request("removed-unrelated")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    fs::write(&path, unrelated).unwrap();
    store.submit(request("restored-unrelated")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let stats = store.index_summary().unwrap().unwrap().scan_stats.unwrap();
    assert_eq!(stats.nfo_seen, 1);
    assert_eq!(stats.nfo_reparsed, 1);
}
#[test]
fn operation_replay_and_changed_input_conflict() {
    let (_temp, store, config) = setup();
    let r = request("same");
    let task = store.submit(r.clone()).unwrap();
    assert_eq!(store.submit(r).unwrap(), task);
    let mut changed = request("same");
    changed.space = Space::Tv;
    changed.root_ids = vec!["tv".into()];
    assert_eq!(
        store.submit(changed).unwrap_err().code,
        "operation-conflict"
    );
    assert_eq!(store.tasks().unwrap().len(), 1);
    let mut settings = config;
    settings.locale = Locale::Traditional;
    let saved = store.configure("locale", settings.clone()).unwrap();
    assert_eq!(store.configure("locale", settings).unwrap(), saved);
    assert_eq!(store.task("same").unwrap().locale, Locale::English);
}
#[test]
fn cancel_during_progress_is_not_overwritten_by_worker() {
    let (_temp, store, config) = setup();
    for i in 0..4 {
        fs::write(
            Path::new(&config.roots[0].path).join(format!("{i}.nfo")),
            fixture().1,
        )
        .unwrap();
    }
    store.submit(request("cancel-me")).unwrap();
    store
        .run_next(
            || false,
            |task| {
                if task.processed == 1 {
                    store
                        .control(TaskControl {
                            operation_id: "cancel1".into(),
                            task_id: task.id.clone(),
                            state: TaskState::Cancelled,
                        })
                        .unwrap();
                }
            },
        )
        .unwrap();
    let task = store.task("cancel-me").unwrap();
    assert_eq!(task.state, TaskState::Cancelled);
    assert_eq!(task.processed, 1);
    assert!(store
        .control(TaskControl {
            operation_id: "resume1".into(),
            task_id: task.id,
            state: TaskState::Requested
        })
        .is_err());
}
#[test]
fn pause_resume_and_replayed_pause_do_not_pause_again() {
    let (_temp, store, config) = setup();
    fs::write(Path::new(&config.roots[0].path).join("x.nfo"), fixture().1).unwrap();
    store.submit(request("job")).unwrap();
    let pause = TaskControl {
        operation_id: "pause1".into(),
        task_id: "job".into(),
        state: TaskState::Paused,
    };
    store.control(pause.clone()).unwrap();
    assert!(store.run_next(|| false, |_| {}).unwrap().is_none());
    store
        .control(TaskControl {
            operation_id: "resume1".into(),
            task_id: "job".into(),
            state: TaskState::Requested,
        })
        .unwrap();
    store.control(pause).unwrap();
    assert_eq!(store.task("job").unwrap().state, TaskState::Requested);
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("job").unwrap().state, TaskState::Completed);
}
#[test]
fn invalid_scope_and_optimistic_configuration_revision() {
    let (_temp, store, config) = setup();
    assert!(store
        .submit(ScanRequest {
            root_ids: vec![],
            ..request("empty")
        })
        .is_err());
    let mut cross = request("cross");
    cross.root_ids = vec!["tv".into()];
    assert!(store.submit(cross).is_err());
    let mut changed = config.clone();
    changed.locale = Locale::Traditional;
    store.configure("change", changed).unwrap();
    assert_eq!(
        store.configure("stale", config).unwrap_err().code,
        "configuration-conflict"
    );
}
#[test]
fn offline_root_preserves_index_and_failure_is_locatable() {
    let (temp, store, config) = setup();
    let dir = Path::new(&config.roots[0].path);
    fs::write(dir.join("one.nfo"), fixture().1).unwrap();
    store.submit(request("initial")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    fs::rename(dir, temp.path().join("offline")).unwrap();
    store.submit(request("offline")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("offline").unwrap().state, TaskState::Failed);
    let page = store.query(query(Space::Movie)).unwrap();
    assert!(page.items.iter().any(|i| i.imdb == "tt0064757"));
    assert!(page.items.iter().any(|i| i
        .error
        .as_ref()
        .is_some_and(|e| e.path.as_deref() == Some(&config.roots[0].path))));
}
#[test]
fn malformed_and_dtd_never_repair_source() {
    let (_temp, store, config) = setup();
    let dir = Path::new(&config.roots[0].path);
    for (name,raw) in [("broken.nfo","<movie>"),("entity.nfo","<!DOCTYPE movie [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><movie><title>&x;</title></movie>")] {fs::write(dir.join(name),raw).unwrap();}
    store.submit(request("errors")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("errors").unwrap().errors, 2);
    assert_eq!(
        fs::read_to_string(dir.join("broken.nfo")).unwrap(),
        "<movie>"
    );
}
#[test]
fn shutdown_is_recoverable_and_history_persists() {
    let (temp, store, _config) = setup();
    store.submit(request("stop")).unwrap();
    store.run_next(|| true, |_| {}).unwrap();
    assert_eq!(store.task("stop").unwrap().state, TaskState::Interrupted);
    drop(store);
    let reopened = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(reopened.task("stop").unwrap().state, TaskState::Interrupted);
}
#[cfg(unix)]
#[test]
fn symlink_is_rejected_instead_of_scanning_external_root() {
    let (_temp, store, config) = setup();
    std::os::unix::fs::symlink(
        &config.roots[1].path,
        Path::new(&config.roots[0].path).join("escape"),
    )
    .unwrap();
    store.submit(request("links")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.task("links").unwrap().errors, 1);
}
#[test]
fn future_database_is_not_downgraded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("future.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(Store::open(&path).is_err());
}

#[test]
fn database_has_one_live_owner() {
    let (temp, store, _) = setup();
    assert!(Store::open(&temp.path().join("state.sqlite")).is_err());
    drop(store);
    assert!(Store::open(&temp.path().join("state.sqlite")).is_ok());
}

#[test]
fn resumed_scan_removes_files_deleted_while_paused() {
    let (_temp, store, config) = setup();
    let path = Path::new(&config.roots[0].path).join("gone.nfo");
    fs::write(&path, fixture().1).unwrap();
    store.submit(request("resume-scan")).unwrap();
    store
        .run_next(
            || false,
            |t| {
                if t.processed == 1 {
                    store
                        .control(TaskControl {
                            operation_id: "pause-delete".into(),
                            task_id: t.id.clone(),
                            state: TaskState::Paused,
                        })
                        .unwrap();
                }
            },
        )
        .unwrap();
    assert_eq!(store.task("resume-scan").unwrap().state, TaskState::Paused);
    fs::remove_file(path).unwrap();
    store
        .control(TaskControl {
            operation_id: "resume-delete".into(),
            task_id: "resume-scan".into(),
            state: TaskState::Requested,
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 0);
    assert_eq!(store.task("resume-scan").unwrap().attempt, 2);
}

#[test]
fn long_unicode_paths_and_incremental_content_change() {
    let (_temp, store, config) = setup();
    let directory = Path::new(&config.roots[0].path)
        .join("中文目录".repeat(10))
        .join("更多目录".repeat(10));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("大写.NFO");
    fs::write(&path, fixture().1).unwrap();
    store.submit(request("long-initial")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let old = store.query(query(Space::Movie)).unwrap().items.remove(0);
    fs::write(
        &path,
        fixture().1.replace("电影 &amp; 测试", "修改后的片名"),
    )
    .unwrap();
    store.submit(request("long-refresh")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let new = store.query(query(Space::Movie)).unwrap().items.remove(0);
    assert_eq!(old.id, new.id);
    assert_ne!(old.source_hash, new.source_hash);
    assert_eq!(new.title, "修改后的片名");
}

#[test]
fn operation_results_are_queryable_and_ids_cannot_cross_operation_kinds() {
    let (temp, store, config) = setup();
    assert_eq!(
        store.operation_result("unknown").unwrap_err().code,
        "operation-not-found"
    );
    assert!(matches!(
        store.operation_result("setup").unwrap(),
        OperationResult::Configuration(_)
    ));
    assert_eq!(
        store.submit(request("setup")).unwrap_err().code,
        "operation-conflict"
    );
    store.submit(request("scan-operation")).unwrap();
    assert_eq!(
        store.configure("scan-operation", config).unwrap_err().code,
        "operation-conflict"
    );
    assert_eq!(
        store
            .control(TaskControl {
                operation_id: "scan-operation".into(),
                task_id: "scan-operation".into(),
                state: TaskState::Paused
            })
            .unwrap_err()
            .code,
        "operation-conflict"
    );
    store
        .control(TaskControl {
            operation_id: "pause-operation".into(),
            task_id: "scan-operation".into(),
            state: TaskState::Paused,
        })
        .unwrap();
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    for id in ["scan-operation", "pause-operation"] {
        let OperationResult::Task(task) = store.operation_result(id).unwrap() else {
            panic!("Expected task result")
        };
        assert_eq!(task.state, TaskState::Paused);
    }
}

#[cfg(windows)]
#[test]
fn windows_verbatim_root_and_drive_relative_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    assert_eq!(paths::checked(&root).unwrap(), root);
    let file = root.join("中文.nfo");
    fs::write(&file, "<movie/>").unwrap();
    assert_eq!(paths::within(&root, &file).unwrap(), file);
    assert_eq!(
        paths::checked(Path::new(r"C:relative")).unwrap_err().code,
        "invalid-path"
    );
    // PathBuf::join normalizes .. for verbatim Windows paths before validation.
    let mut traversal = root.as_os_str().to_os_string();
    traversal.push(r"\..");
    assert_eq!(
        paths::checked(Path::new(&traversal)).unwrap_err().code,
        "ambiguous-path"
    );
}

#[test]
fn one_physical_root_keeps_movie_tv_scopes_and_errors_independent() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let media = base.join("mixed");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(
        media.join("movie.nfo"),
        b"<movie><title>Movie only</title></movie>",
    )
    .unwrap();
    std::fs::write(
        media.join("tvshow.nfo"),
        b"<tvshow><title>TV only</title></tvshow>",
    )
    .unwrap();
    std::fs::write(media.join("broken.nfo"), b"<broken").unwrap();
    let store = Store::open(&base.join("db.sqlite")).unwrap();
    store
        .configure(
            "mixed",
            Configuration {
                roots: vec![
                    LibraryRoot {
                        id: "movie".into(),
                        path: media.to_string_lossy().into(),
                        space: Space::Movie,
                    },
                    LibraryRoot {
                        id: "tv".into(),
                        path: media.to_string_lossy().into(),
                        space: Space::Tv,
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();
    for (id, space) in [("movie", Space::Movie), ("tv", Space::Tv)] {
        store
            .submit(ScanRequest {
                operation_id: format!("scan-{id}"),
                space,
                root_ids: vec![id.into()],
            })
            .unwrap();
        store.run_next(|| false, |_| {}).unwrap();
    }
    let movie = store.query(query(Space::Movie)).unwrap();
    let tv = store.query(query(Space::Tv)).unwrap();
    assert_eq!(movie.total, 2);
    assert_eq!(tv.total, 2);
    assert!(movie.items.iter().any(|i| i.title == "Movie only"));
    assert!(tv.items.iter().any(|i| i.title == "TV only"));
    assert_ne!(
        movie.items.iter().find(|i| i.error.is_some()).unwrap().id,
        tv.items.iter().find(|i| i.error.is_some()).unwrap().id
    );
    std::fs::remove_file(media.join("broken.nfo")).unwrap();
    store
        .submit(ScanRequest {
            operation_id: "refresh-movie".into(),
            space: Space::Movie,
            root_ids: vec!["movie".into()],
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(store.query(query(Space::Movie)).unwrap().total, 1);
    assert_eq!(store.query(query(Space::Tv)).unwrap().total, 2);
}

#[test]
fn index_update_time_survives_restart_and_cancel_does_not_claim_a_new_index() {
    let (temp, store, config) = setup();
    let path = Path::new(&config.roots[0].path).join("movie.nfo");
    fs::write(&path, fixture().1).unwrap();
    let original = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(store.catalog_summary().unwrap().generated_at.is_none());
    assert!(store.catalog_summary().unwrap().roots_configured);
    store.submit(request("dated-scan")).unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    let at = store.catalog_summary().unwrap().generated_at.unwrap();
    chrono::DateTime::parse_from_rfc3339(&at).unwrap();
    assert_eq!(at.split_once('.').unwrap().1.len(), 8);
    drop(store);
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    assert_eq!(
        store.catalog_summary().unwrap().generated_at.as_deref(),
        Some(at.as_str())
    );
    store.submit(request("cancelled-scan")).unwrap();
    store.run_next(|| true, |_| {}).unwrap();
    assert_eq!(
        store.catalog_summary().unwrap().generated_at.as_deref(),
        Some(at.as_str())
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
}
