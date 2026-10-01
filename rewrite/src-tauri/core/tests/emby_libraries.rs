use std::fs;
use tcm_core::{emby_libraries::*, store::Store, Space};
#[test]
fn mappings_use_component_boundaries_longest_prefix_and_preserve_unicode() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let local = root.to_string_lossy().to_string();
    let mappings = vec![PathMapping {
        server_prefix: r"D:\Media".into(),
        local_root: local.clone(),
    }];
    assert_eq!(
        mapped_path(r"d:\MEDIA\中文 A\movie.nfo", &mappings).unwrap(),
        root.join("中文 A/movie.nfo")
    );
    assert!(mapped_path(r"D:\Media\..\outside", &mappings).is_err());
    assert_ne!(
        mapped_path(r"D:\MediaOther\movie.nfo", &mappings).ok(),
        Some(root.join("Other/movie.nfo"))
    );
    let mut duplicate = mappings.clone();
    duplicate.push(mappings[0].clone());
    assert!(mapped_path(r"D:\Media\file", &duplicate).is_err());
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let mut longest = mappings;
    longest.push(PathMapping {
        server_prefix: r"D:\Media\Movies".into(),
        local_root: nested.to_string_lossy().into(),
    });
    assert_eq!(
        mapped_path(r"D:\Media\Movies\a.nfo", &longest).unwrap(),
        nested.join("a.nfo")
    );
}
#[test]
fn physical_roots_under_virtual_wrappers_are_discovered_without_changing_database_or_nfo() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let media = root.join("电影");
    fs::create_dir(&media).unwrap();
    let nfo = media.join("movie.nfo");
    fs::write(
        &nfo,
        b"<movie><title>A</title><uniqueid type=\"imdb\">tt0061452</uniqueid></movie>",
    )
    .unwrap();
    let before = fs::read(&nfo).unwrap();
    let modified = fs::metadata(&nfo).unwrap().modified().unwrap();
    let path = root.join("data/library.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(2,0,'Global','',3,NULL,NULL),(10,2,'Wrapper','',3,NULL,NULL),(11,10,'Movies','D:\\Media',3,NULL,NULL),(12,11,'Film','D:\\Media\\movie.mkv',5,'imdb=tt0061452',NULL),(20,2,'Offline TV','Z:\\TV',3,NULL,NULL),(21,20,'Series','Z:\\TV\\show',6,NULL,NULL); ").unwrap();
    drop(db);
    let database_before = fs::read(&path).unwrap();
    let libraries = discover(
        &root,
        &[PathMapping {
            server_prefix: r"D:\Media".into(),
            local_root: media.to_string_lossy().into(),
        }],
    )
    .unwrap();
    assert_eq!(libraries.len(), 2);
    assert_eq!(libraries[0].spaces, vec![Space::Movie]);
    assert_eq!(libraries[0].state, "ready");
    assert_eq!(libraries[0].parent_id.as_deref(), Some("10"));
    assert_eq!(libraries[0].evidence, "IMDb-video=1");
    assert_eq!(libraries[0].probe_nfos, None);
    assert_eq!(libraries[1].spaces, vec![Space::Tv]);
    assert_eq!(libraries[1].state, "offline-or-needs-mapping");
    assert_eq!(libraries[1].evidence, "Series=1");
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&nfo).unwrap(), before);
    assert_eq!(fs::metadata(&nfo).unwrap().modified().unwrap(), modified);
}
#[test]
fn incompatible_database_schema_fails_without_modifying_it() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let path = root.join("data/library.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER, Name TEXT)")
        .unwrap();
    drop(db);
    let bytes = fs::read(&path).unwrap();
    let failure = discover(&root, &[]).unwrap_err();
    assert_eq!(failure.code, "emby-library-discovery");
    assert_eq!(fs::read(path).unwrap(), bytes);
}
#[test]
fn mapping_changes_are_queryable_replayable_and_revision_guarded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite");
    let store = Store::open(&path).unwrap();
    let value = MappingSettings {
        revision: 0,
        mappings: vec![PathMapping {
            server_prefix: "/media".into(),
            local_root: temp.path().canonicalize().unwrap().to_string_lossy().into(),
        }],
    };
    assert_eq!(
        store
            .set_path_mappings("mapping", value.clone())
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        store
            .set_path_mappings("mapping", value.clone())
            .unwrap()
            .revision,
        1
    );
    assert!(store.set_path_mappings("stale", value).is_err());
    drop(store);
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .path_mappings()
            .unwrap()
            .mappings
            .len(),
        1
    );
}

#[test]
fn discovery_snapshot_survives_restart_offline_roots_and_failed_queries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("emby");
    fs::create_dir_all(data.join("data")).unwrap();
    let db_path = data.join("data/library.db");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(20,2,'Offline TV','Z:\\TV',3,NULL,NULL),(21,20,'Series','Z:\\TV\\show',6,NULL,NULL);").unwrap();
    let state = root.join("manager.sqlite");
    let store = Store::open(&state).unwrap();
    let data = data.to_str().unwrap();
    assert!(store.discovered_libraries(data).unwrap().is_empty());
    let first = store.discover_libraries(data).unwrap();
    assert_eq!(first[0].spaces, vec![Space::Tv]);
    assert!(store.configuration().unwrap().roots.is_empty());
    assert!(store.folder_settings().unwrap().folders.is_empty());
    drop(store);
    db.execute("DELETE FROM MediaItems WHERE Id=21", [])
        .unwrap();
    let store = Store::open(&state).unwrap();
    assert_eq!(
        store.discovered_libraries(data).unwrap()[0].spaces,
        vec![Space::Tv]
    );
    assert_eq!(
        store.discover_libraries(data).unwrap()[0].spaces,
        vec![Space::Tv]
    );
    assert!(store
        .discovered_libraries("another installation")
        .unwrap()
        .is_empty());
    db.execute_batch("DROP TABLE MediaItems").unwrap();
    let bytes = fs::read(&db_path).unwrap();
    assert!(store.discover_libraries(data).is_err());
    assert_eq!(
        store.discovered_libraries(data).unwrap()[0].spaces,
        vec![Space::Tv]
    );
    assert_eq!(fs::read(&db_path).unwrap(), bytes);
    store
        .set_path_mappings(
            "different-mapping",
            MappingSettings {
                revision: 0,
                mappings: vec![PathMapping {
                    server_prefix: r"Z:\TV".into(),
                    local_root: root.to_string_lossy().into(),
                }],
            },
        )
        .unwrap();
    assert!(store.discovered_libraries(data).unwrap().is_empty());
}

#[test]
fn offline_classification_does_not_follow_an_id_to_a_different_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let db = rusqlite::Connection::open(root.join("data/library.db")).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(20,2,'TV','Z:\\TV',3,NULL,NULL),(21,20,'Series','Z:\\TV\\show',6,NULL,NULL);").unwrap();
    let previous = discover(&root, &[]).unwrap();
    db.execute_batch(
        "DELETE FROM MediaItems WHERE Id=21; UPDATE MediaItems SET Path='Z:\\Other' WHERE Id=20;",
    )
    .unwrap();
    assert!(discover_with_previous(&root, &[], &previous).unwrap()[0]
        .spaces
        .is_empty());
}

#[test]
fn nfo_probe_reports_actual_file_count_and_original_evidence_order() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    fs::write(
        media.join("movie.nfo"),
        b"<movie><title>A</title><uniqueid type=\"imdb\">tt0061452</uniqueid></movie>",
    )
    .unwrap();
    fs::write(
        media.join("tvshow.nfo"),
        b"<tvshow><title>B</title></tvshow>",
    )
    .unwrap();
    let empty = root.join("empty");
    fs::create_dir(&empty).unwrap();
    let db = rusqlite::Connection::open(root.join("data/library.db")).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(10,2,'Mixed','D:\\Mixed',3,NULL,NULL),(11,10,'Unclassified','D:\\Mixed\\video.mkv',5,NULL,NULL),(20,2,'Ignored','D:\\Empty',3,NULL,NULL),(21,20,'Unclassified','D:\\Empty\\video.mkv',5,NULL,NULL);").unwrap();
    let rows = discover(
        &root,
        &[
            PathMapping {
                server_prefix: r"D:\Mixed".into(),
                local_root: media.to_string_lossy().into(),
            },
            PathMapping {
                server_prefix: r"D:\Empty".into(),
                local_root: empty.to_string_lossy().into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(rows[0].spaces, vec![Space::Movie, Space::Tv]);
    assert_eq!(rows[0].probe_nfos, Some(2));
    assert_eq!(rows[0].evidence, "NFO=movie, NFO=tv");
    assert_eq!(rows[1].probe_nfos, Some(0));
    assert_eq!(rows[1].evidence, "NFO-probe=0");
    assert!(rows[1].spaces.is_empty());
    let mut old = serde_json::to_value(&rows[0]).unwrap();
    for key in ["parent_id", "evidence", "probe_nfos"] {
        old.as_object_mut().unwrap().remove(key);
    }
    let restored: DiscoveredLibrary = serde_json::from_value(old).unwrap();
    assert_eq!(restored.parent_id, None);
    assert!(restored.evidence.is_empty());
    assert_eq!(restored.spaces, rows[0].spaces);
}

#[test]
fn original_discovery_file_is_readonly_bound_to_its_emby_data_and_replaced_by_new_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("custom-tech-specs")).unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let path = root.join("custom-tech-specs/manager-root-discovery.json");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(br#"{"generatedAt":"old","libraries":[{"Id":20,"ParentId":2,"Name":"TV","Path":"Z:\\TV","Kind":"TV","Included":true,"Online":false,"SeriesCount":1,"Evidence":"Series=1"},{"Id":30,"Path":"Z:\\Music","Kind":"Ignored","Included":false},{"Included":true,"Path":""}]}"#);
    fs::write(&path, &bytes).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let store = Store::open(&root.join("manager.sqlite")).unwrap();
    let rows = store.discovered_libraries(root.to_str().unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].server_path, r"Z:\TV");
    assert_eq!(rows[0].spaces, vec![Space::Tv]);
    assert_eq!(rows[0].parent_id.as_deref(), Some("2"));
    assert!(rows[0].local_path.is_none());
    assert!(store.configuration().unwrap().roots.is_empty());
    assert!(store.tasks().unwrap().is_empty());
    assert!(store.folder_settings().unwrap().folders.is_empty());
    assert!(store
        .discovered_libraries(root.join("another-emby").to_str().unwrap())
        .unwrap()
        .is_empty());
    let db = rusqlite::Connection::open(root.join("data/library.db")).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT); INSERT INTO MediaItems VALUES(20,2,'TV','Z:\\TV',3,NULL,NULL);").unwrap();
    let queried = store.discover_libraries(root.to_str().unwrap()).unwrap();
    assert_eq!(queried[0].spaces, vec![Space::Tv]);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fs::write(&path, b"{invalid").unwrap();
    assert_eq!(
        store.discovered_libraries(root.to_str().unwrap()).unwrap()[0].spaces,
        vec![Space::Tv]
    );
    drop(store);
    let store = Store::open(&root.join("manager.sqlite")).unwrap();
    assert_eq!(
        store.discovered_libraries(root.to_str().unwrap()).unwrap()[0].spaces,
        vec![Space::Tv]
    );
}

#[test]
fn damaged_or_ambiguous_original_discovery_is_not_reported_as_an_empty_success() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("custom-tech-specs")).unwrap();
    let path = root.join("custom-tech-specs/manager-root-discovery.json");
    let store = Store::open(&root.join("manager.sqlite")).unwrap();
    for (bytes,code) in [(b"{invalid".as_slice(),"legacy-discovery-invalid"),(br#"{"libraries":[{"Included":true,"Path":"Z:\\TV","Kind":"unknown"}]}"#.as_slice(),"legacy-discovery-kind"),(br#"{"libraries":[{"Included":true,"Path":"Z:\\TV","Kind":"TV"},{"Included":true,"Path":"z:\\tv\\","Kind":"TV"}]}"#.as_slice(),"legacy-discovery-path")]{
        fs::write(&path,bytes).unwrap();assert_eq!(store.discovered_libraries(root.to_str().unwrap()).unwrap_err().code,code);assert_eq!(fs::read(&path).unwrap(),bytes);
    }
    fs::create_dir(root.join("data")).unwrap();
    let db = rusqlite::Connection::open(root.join("data/library.db")).unwrap();
    db.execute_batch("CREATE TABLE MediaItems(Id INTEGER,ParentId INTEGER,Name TEXT,Path TEXT,type INTEGER,ProviderIds TEXT,ExtraType TEXT);").unwrap();
    let original = fs::read(&path).unwrap();
    assert!(store
        .discover_libraries(root.to_str().unwrap())
        .unwrap()
        .is_empty());
    assert!(store
        .discovered_libraries(root.to_str().unwrap())
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn original_online_snapshot_requires_a_local_path_and_keeps_mixed_classification() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("custom-tech-specs")).unwrap();
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    let snapshot = serde_json::json!({"libraries":[
        {"Id":1,"Path":"Z:\\Unavailable","Kind":"Movies","Included":true,"Online":true},
        {"Id":2,"Path":media,"Kind":"Mixed Movie/TV","Included":true,"Online":true}
    ]});
    fs::write(
        root.join("custom-tech-specs/manager-root-discovery.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let store = Store::open(&root.join("manager.sqlite")).unwrap();
    let rows = store.discovered_libraries(root.to_str().unwrap()).unwrap();
    assert!(rows[0].local_path.is_none());
    assert_eq!(rows[0].state, "offline-or-needs-mapping");
    assert_eq!(rows[1].local_path.as_deref(), media.to_str());
    assert_eq!(rows[1].spaces, vec![Space::Movie, Space::Tv]);
    assert_eq!(rows[1].state, "mixed-requires-scope-review");
    assert!(store.configuration().unwrap().roots.is_empty());
}

#[cfg(unix)]
#[test]
fn original_discovery_symlink_cannot_read_another_installations_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("emby");
    fs::create_dir_all(data.join("custom-tech-specs")).unwrap();
    let outside = root.join("outside.json");
    fs::write(&outside, b"{\"libraries\":[]}").unwrap();
    std::os::unix::fs::symlink(
        &outside,
        data.join("custom-tech-specs/manager-root-discovery.json"),
    )
    .unwrap();
    let store = Store::open(&root.join("manager.sqlite")).unwrap();
    assert_eq!(
        store
            .discovered_libraries(data.to_str().unwrap())
            .unwrap_err()
            .code,
        "ambiguous-path"
    );
}
