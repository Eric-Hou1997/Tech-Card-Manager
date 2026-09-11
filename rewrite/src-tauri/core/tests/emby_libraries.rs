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
        "4.9.5.0",
        &[PathMapping {
            server_prefix: r"D:\Media".into(),
            local_root: media.to_string_lossy().into(),
        }],
    )
    .unwrap();
    assert_eq!(libraries.len(), 2);
    assert_eq!(libraries[0].spaces, vec![Space::Movie]);
    assert_eq!(libraries[0].state, "ready");
    assert_eq!(libraries[1].spaces, vec![Space::Tv]);
    assert_eq!(libraries[1].state, "offline-or-needs-mapping");
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&nfo).unwrap(), before);
    assert_eq!(fs::metadata(&nfo).unwrap().modified().unwrap(), modified);
    assert!(discover(&root, "5.0.0.0", &[]).is_err());
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
