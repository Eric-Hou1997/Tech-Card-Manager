use std::fs;
use tcm_core::emby_environment::*;
#[test]
fn discovers_windows_layout_without_writing_and_keeps_data_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let appdata = root.join("漫游目录");
    let emby = appdata.join("Emby-Server");
    let web = emby.join("system/dashboard-ui");
    fs::create_dir_all(&web).unwrap();
    fs::create_dir_all(emby.join("programdata/config")).unwrap();
    let html = format!(
        "<html><head></head><body>{}</body></html>",
        "Emby".repeat(100)
    );
    fs::write(web.join("index.html"), &html).unwrap();
    let before = fs::metadata(web.join("index.html"))
        .unwrap()
        .modified()
        .unwrap();
    let found = discover("windows", &root, Some(&appdata), Some(&appdata));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].web, web.to_string_lossy());
    assert!(found[0].data.as_ref().unwrap().ends_with("programdata"));
    assert!(found[0].version.is_none());
    assert_eq!(fs::read_to_string(web.join("index.html")).unwrap(), html);
    assert_eq!(
        fs::metadata(web.join("index.html"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(fs::read_dir(&web).unwrap().count(), 1);
}
#[test]
fn server_version_is_actual_observation_and_unsupported_is_not_accepted() {
    let mut e = Environment {
        issues: vec!["server-version-unverified".into()],
        ..Default::default()
    };
    server_version(&mut e, &serde_json::json!({"Version":"4.9.5.0"})).unwrap();
    assert!(e.issues.is_empty());
    server_version(&mut e, &serde_json::json!({"Version":"4.10.0.0"})).unwrap();
    assert!(e.issues.contains(&"server-version-not-accepted".into()));
    assert!(server_version(&mut e, &serde_json::json!({"Version":"fake"})).is_err());
    assert!(endpoint("http://user:password@localhost:8096").is_err());
    assert!(endpoint("file:///tmp/Emby").is_err());
    assert_eq!(
        endpoint("https://example.org/prefix/").unwrap().as_str(),
        "https://example.org/prefix/emby/System/Info/Public"
    );
}
