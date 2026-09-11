use std::fs;
use tcm_core::{emby::*, library, LibraryRoot, Space};
fn html() -> Vec<u8> {
    format!(
        "\u{feff}<html><head><title>测试</title></head>\r\n<body>{}</body></html>",
        "Emby 内容 ".repeat(40)
    )
    .into_bytes()
}
fn index() -> PublicIndex {
    public_index(&[], "2026-09-11T00:00:00Z".into())
}
fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let web = root.join("web");
    let backup = root.join("private");
    fs::create_dir(&web).unwrap();
    fs::write(web.join("index.html"), html()).unwrap();
    (temp, web, backup)
}
#[test]
fn install_replay_lease_stop_remove_preserve_original_and_unowned_files() {
    let (_temp, web, backup) = setup();
    fs::write(web.join("other-plugin.js"), "external").unwrap();
    let integration = Integration::open(&web, &backup).unwrap();
    let plan = integration
        .plan("install", "install", b"card-js", &index(), b"{}")
        .unwrap();
    assert!(!integration.status().unwrap().installed);
    assert!(
        integration
            .apply(&plan.id, &plan.fingerprint)
            .unwrap()
            .healthy
    );
    assert!(
        integration
            .apply(&plan.id, &plan.fingerprint)
            .unwrap()
            .healthy
    );
    assert_eq!(
        clean_index(&fs::read(web.join("index.html")).unwrap()).unwrap(),
        html()
    );
    let lease = integration.begin_session("session-1").unwrap();
    assert!(lease.enabled);
    let stopped = integration
        .renew_session("session-1", lease.sequence + 1, false)
        .unwrap();
    assert_eq!(stopped.updated_at, stopped.expires_at);
    assert!(integration.renew_session("other-session", 3, true).is_err());
    let plan = integration
        .plan("remove", "remove", b"card-js", &index(), b"{}")
        .unwrap();
    assert!(
        !integration
            .apply(&plan.id, &plan.fingerprint)
            .unwrap()
            .installed
    );
    assert_eq!(fs::read(web.join("index.html")).unwrap(), html());
    assert!(!web.join("technical-specs-card.js").exists());
    assert!(!web.join("technical-specs-runtime.json").exists());
    assert_eq!(
        fs::read_to_string(web.join("other-plugin.js")).unwrap(),
        "external"
    );
}
#[test]
fn shared_lock_and_reviewed_snapshot_conflicts_fail_without_overwrite() {
    let (_temp, web, backup) = setup();
    let integration = Integration::open(&web, &backup).unwrap();
    assert!(Integration::open(&web, &backup.with_file_name("other-private")).is_err());
    let plan = integration
        .plan("one", "install", b"js", &index(), b"{}")
        .unwrap();
    let mut outside = html();
    outside.extend_from_slice(b"external-edit");
    fs::write(web.join("index.html"), &outside).unwrap();
    assert!(integration.apply(&plan.id, &plan.fingerprint).is_err());
    assert_eq!(fs::read(web.join("index.html")).unwrap(), outside);
    assert!(!web.join("technical-specs-card.js").exists());
    assert!(integration
        .plan("one", "remove", b"js", &index(), b"{}")
        .is_err());
}
#[test]
fn unknown_marker_and_asset_fail_closed() {
    let (_temp, web, backup) = setup();
    let integration = Integration::open(&web, &backup).unwrap();
    fs::write(web.join("technical-specs-card.js"), "external").unwrap();
    assert!(integration
        .plan("one", "install", b"js", &index(), b"{}")
        .is_err());
    let malformed=String::from_utf8(html()).unwrap().replace("</body>","<!-- IMDbTechManager WebPatch BEGIN -->external<!-- IMDbTechManager WebPatch END --></body>");
    assert!(clean_index(malformed.as_bytes()).is_err());
}
#[test]
fn emby_upgrade_repair_uses_new_index_and_preserves_upgrade_bytes() {
    let (_temp, web, backup) = setup();
    let integration = Integration::open(&web, &backup).unwrap();
    let p = integration
        .plan("install", "install", b"js", &index(), b"{}")
        .unwrap();
    integration.apply(&p.id, &p.fingerprint).unwrap();
    let upgraded = String::from_utf8(html())
        .unwrap()
        .replace("测试", "升级后的 Emby")
        .into_bytes();
    fs::write(web.join("index.html"), &upgraded).unwrap();
    assert!(!integration.status().unwrap().healthy);
    let p = integration
        .plan("repair", "repair", b"js", &index(), b"{}")
        .unwrap();
    integration.apply(&p.id, &p.fingerprint).unwrap();
    let p = integration
        .plan("remove", "remove", b"js", &index(), b"{}")
        .unwrap();
    integration.apply(&p.id, &p.fingerprint).unwrap();
    assert_eq!(fs::read(web.join("index.html")).unwrap(), upgraded);
}
#[test]
fn public_feed_is_path_free_merges_series_and_excludes_episodes() {
    let (_temp, web, _) = setup();
    let root = LibraryRoot {
        id: "root".into(),
        space: Space::Movie,
        path: web.to_string_lossy().into(),
    };
    let mut items = Vec::new();
    for (name, kind, camera) in [
        ("a.nfo", "movie", "ARRI"),
        ("b.nfo", "tvshow", "arri"),
        ("c.nfo", "episodedetails", "SecretEpisodeCamera"),
    ] {
        let path = web.join(name);
        let nfo=format!("<{kind}><title>测试</title><uniqueid type=\"imdb\">tt1234567</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>{camera}</item></section></technicalspecs></{kind}>");
        fs::write(&path, nfo).unwrap();
        items.push(library::read(&root, &path).unwrap());
    }
    let public = public_index(&items, "2026-09-11T00:00:00Z".into());
    let json = serde_json::to_string(&public).unwrap();
    assert!(!json.contains(web.to_str().unwrap()));
    assert!(!json.contains("SecretEpisodeCamera"));
    assert_eq!(public.item_types["tt1234567"], "Series");
}

#[test]
fn stop_joins_renewal_and_restart_gets_new_session() {
    use std::{sync::Arc, time::Duration};
    use tcm_core::card_service::CardService;
    let (_temp, web, backup) = setup();
    let target = Arc::new(Integration::open(&web, &backup).unwrap());
    let p = target
        .plan("install", "install", b"js", &index(), b"{}")
        .unwrap();
    target.apply(&p.id, &p.fingerprint).unwrap();
    let mut service = CardService::start(target.clone(), "one").unwrap();
    std::thread::sleep(Duration::from_millis(2200));
    assert!(service.status().unwrap().lease.unwrap().sequence >= 2);
    service.stop().unwrap();
    let stopped = fs::read(web.join("technical-specs-runtime.json")).unwrap();
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(
        stopped,
        fs::read(web.join("technical-specs-runtime.json")).unwrap()
    );
    let mut restarted = CardService::start(target, "two").unwrap();
    assert_eq!(restarted.status().unwrap().lease.unwrap().session_id, "two");
    restarted.stop().unwrap();
}

#[test]
fn explicit_legacy_adoption_requires_baseline_and_preserves_unrelated_assets() {
    let (_temp, web, backup) = setup();
    let patched=String::from_utf8(html()).unwrap().replace("</body>","<!-- IMDbTechManager WebPatch BEGIN --><script src=\"technical-specs-card.js?v=4.1.0\"></script><!-- IMDbTechManager WebPatch END --></body>");
    fs::write(web.join("index.html"), patched).unwrap();
    fs::write(web.join("technical-specs-card.js"), b"baseline-js").unwrap();
    fs::write(
        web.join("technical-specs-data.json"),
        serde_json::to_vec(&index()).unwrap(),
    )
    .unwrap();
    fs::write(web.join("external.js"), b"external").unwrap();
    let integration = Integration::open(&web, &backup).unwrap();
    assert!(integration
        .plan("bad", "adopt", b"different-js", &index(), b"{}")
        .is_err());
    let plan = integration
        .plan(
            "adopt",
            "adopt",
            b"baseline-js",
            &index(),
            &bundled_card_languages().unwrap(),
        )
        .unwrap();
    assert!(plan.legacy_patch);
    assert!(
        integration
            .apply(&plan.id, &plan.fingerprint)
            .unwrap()
            .healthy
    );
    assert!(integration.begin_session("new-manager").unwrap().enabled);
    assert_eq!(fs::read(web.join("external.js")).unwrap(), b"external");
}
#[test]
fn card_language_publication_contains_all_five_presentation_packs() {
    let data: serde_json::Value =
        serde_json::from_slice(&bundled_card_languages().unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["languages"].as_object().unwrap().len(), 5);
    for locale in ["fr-FR", "ru-RU", "ja-JP", "es-ES", "th-TH"] {
        let messages = data["languages"][locale].as_object().unwrap();
        assert_eq!(messages.len(), 12);
        assert!(messages
            .iter()
            .all(|(key, value)| key.starts_with("legacy.")
                && value.as_str().is_some_and(|s| !s.is_empty())));
    }
}
#[test]
fn index_refresh_commits_only_changed_public_data_and_preserves_active_lease() {
    let (_temp, web, backup) = setup();
    let integration = Integration::open(&web, &backup).unwrap();
    let plan = integration
        .plan("install", "install", b"js", &index(), b"{}")
        .unwrap();
    integration.apply(&plan.id, &plan.fingerprint).unwrap();
    let lease = integration.begin_session("live").unwrap();
    let lease_bytes = fs::read(web.join("technical-specs-runtime.json")).unwrap();
    let initial = fs::metadata(web.join("technical-specs-data.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert!(!integration.publish_index(&index()).unwrap());
    assert_eq!(
        fs::metadata(web.join("technical-specs-data.json"))
            .unwrap()
            .modified()
            .unwrap(),
        initial
    );
    let mut updated = index();
    updated.items.insert(
        "tt0061452".into(),
        [("Camera".into(), vec!["New camera".into()])].into(),
    );
    updated
        .item_types
        .insert("tt0061452".into(), "Movie".into());
    assert!(integration.publish_index(&updated).unwrap());
    assert!(integration.status().unwrap().healthy);
    let published: PublicIndex =
        serde_json::from_slice(&fs::read(web.join("technical-specs-data.json")).unwrap()).unwrap();
    assert_eq!(published.items, updated.items);
    assert_eq!(
        fs::read(web.join("technical-specs-runtime.json")).unwrap(),
        lease_bytes
    );
    integration
        .renew_session("live", lease.sequence + 1, false)
        .unwrap();
    fs::write(web.join("technical-specs-data.json"), b"external").unwrap();
    assert!(integration.publish_index(&index()).is_err());
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        b"external"
    );
}

#[test]
fn changed_card_asset_changes_browser_cache_identity_and_removal_restores_original() {
    let (_temp, web, backup) = setup();
    let integration = Integration::open(&web, &backup).unwrap();
    let first = integration
        .plan("first", "install", b"card-one", &index(), b"{}")
        .unwrap();
    integration.apply(&first.id, &first.fingerprint).unwrap();
    let old = fs::read(web.join("index.html")).unwrap();
    let next = integration
        .plan("next", "update", b"card-two", &index(), b"{}")
        .unwrap();
    integration.apply(&next.id, &next.fingerprint).unwrap();
    let new = fs::read(web.join("index.html")).unwrap();
    assert_ne!(old, new);
    assert_eq!(clean_index(&new).unwrap(), html());
    assert_eq!(
        fs::read(web.join("technical-specs-card.js")).unwrap(),
        b"card-two"
    );
    let remove = integration
        .plan("remove", "remove", b"card-two", &index(), b"{}")
        .unwrap();
    integration.apply(&remove.id, &remove.fingerprint).unwrap();
    assert_eq!(fs::read(web.join("index.html")).unwrap(), html());
}
