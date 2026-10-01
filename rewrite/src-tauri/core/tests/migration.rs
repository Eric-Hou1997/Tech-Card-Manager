use std::fs;
use tcm_core::{store::Store, *};
fn setup() -> (tempfile::TempDir, std::path::PathBuf, Store) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let old = root.join("legacy");
    fs::create_dir(&old).unwrap();
    let store = Store::open(&root.join("new.sqlite")).unwrap();
    (t, old, store)
}
#[test]
fn import_preserves_custom_fields_history_bytes_and_replay_after_source_offline() {
    let (_t, old, store) = setup();
    let media = old.parent().unwrap().join("电影");
    fs::create_dir(&media).unwrap();
    let cfg = serde_json::json!({"library_roots":{"movies":[media],"tv":[]},"roots":["/offline/unassigned"],"ai":{"prompt":"自定义提示词\nKEEP EXACT","model":"custom-model"},"unknown_future_field":{"keep":true}});
    fs::write(old.join("config.json"), serde_json::to_vec(&cfg).unwrap()).unwrap();
    fs::create_dir(old.join("logs")).unwrap();
    let log = b"original\r\n\xff\x00";
    fs::write(old.join("logs/history.log"), log).unwrap();
    let plan = store
        .prepare_migration("import", &old, "itm-engine")
        .unwrap();
    assert_eq!(store.configuration().unwrap().revision, 0);
    let receipt = store.apply_migration("import", &plan.fingerprint).unwrap();
    assert_eq!(receipt.configuration.roots.len(), 1);
    assert_eq!(receipt.pending_roots.len(), 1);
    assert_eq!(
        store.legacy_artifact("import", "logs/history.log").unwrap(),
        log
    );
    assert_eq!(
        store.preferences("legacy:itm-engine").unwrap()["ai"]["prompt"],
        cfg["ai"]["prompt"]
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &store.legacy_artifact("import", "config.json").unwrap()
        )
        .unwrap(),
        cfg
    );
    fs::rename(&old, old.with_file_name("offline")).unwrap();
    assert_eq!(
        store
            .apply_migration("import", &plan.fingerprint)
            .unwrap()
            .configuration
            .revision,
        receipt.configuration.revision
    );
}
#[test]
fn changed_late_file_rolls_back_entire_database_import() {
    let (_t, old, store) = setup();
    fs::write(old.join("config.json"), b"{}").unwrap();
    fs::write(old.join("z-history.json"), b"[]").unwrap();
    let plan = store
        .prepare_migration("import", &old, "itm-engine")
        .unwrap();
    fs::write(old.join("z-history.json"), b"[1]").unwrap();
    assert!(store.apply_migration("import", &plan.fingerprint).is_err());
    assert_eq!(store.configuration().unwrap().revision, 0);
    assert!(store.legacy_artifact("import", "config.json").is_err());
    fs::write(old.join("z-history.json"), b"[]").unwrap();
    assert!(store.apply_migration("import", &plan.fingerprint).is_ok());
}
#[test]
fn migration_id_cannot_be_reused_for_other_operations() {
    let (_t, old, store) = setup();
    fs::write(old.join("config.json"), b"{}").unwrap();
    store
        .prepare_migration("import", &old, "itm-engine")
        .unwrap();
    assert!(store.configure("import", Configuration::default()).is_err());
    assert!(store
        .prepare_migration("import", &old, "itm-manager")
        .is_err());
    assert!(matches!(
        store.operation_result("import").unwrap(),
        OperationResult::MigrationPlan(_)
    ));
}
#[test]
fn credentials_never_enter_generic_legacy_archive() {
    let (_t, old, store) = setup();
    fs::write(
        old.join("config.json"),
        br#"{"ai":{"api_key":"test-only-sentinel"}}"#,
    )
    .unwrap();
    let error = store
        .prepare_migration("import", &old, "itm-engine")
        .unwrap_err();
    assert_eq!(error.code, "migration-credential-boundary");
    assert!(!error.message.contains("test-only-sentinel"));
}
#[test]
fn malformed_json_and_disabled_tcm_roots_remain_recoverable() {
    let (_t, old, store) = setup();
    fs::create_dir(old.join("data")).unwrap();
    fs::write(old.join("data/settings.json"),br#"{"language":"en-US","library_roots":[{"path":"Z:\\offline","kind":"Movie","enabled":false}]}"#).unwrap();
    fs::write(old.join("data/manager-state.json"), b"{interrupted").unwrap();
    let plan = store
        .prepare_migration("import", &old, "tcm-portable")
        .unwrap();
    assert!(!plan.warnings.is_empty());
    let receipt = store.apply_migration("import", &plan.fingerprint).unwrap();
    assert_eq!(receipt.pending_roots[0].state, "disabled");
    assert_eq!(receipt.configuration.locale, Locale::English);
    assert_eq!(
        store
            .legacy_artifact("import", "data/manager-state.json")
            .unwrap(),
        b"{interrupted"
    );
}

#[test]
fn truncated_escaped_credential_json_stays_outside_generic_archive() {
    let (_t, old, store) = setup();
    for (index, bytes) in [
        br#"{"api_key":"test-only-sentinel", "rest": "#.as_slice(),
        br#"{"api\u005fkey":"test-only-sentinel" "#.as_slice(),
        br#"{"authorization":123}"#.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        fs::write(old.join("config.json"), bytes).unwrap();
        let error = store
            .prepare_migration(&format!("secret-{index}"), &old, "itm-engine")
            .unwrap_err();
        assert_eq!(error.code, "migration-credential-boundary");
        assert!(!error.message.contains("sentinel"));
        assert_eq!(fs::read(old.join("config.json")).unwrap(), bytes);
    }
}

#[test]
fn mixed_legacy_root_migrates_both_spaces_without_copying_or_moving_media() {
    let (_temp, old, store) = setup();
    let media = old.parent().unwrap().join("mixed");
    fs::create_dir(&media).unwrap();
    let value = serde_json::json!({"library_roots":[{"path":media,"kind":"mixed","enabled":true}]});
    fs::write(
        old.join("settings.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let plan = store
        .prepare_migration("mixed-import", &old, "tcm-portable")
        .unwrap();
    assert_eq!(plan.roots.len(), 2);
    let receipt = store
        .apply_migration("mixed-import", &plan.fingerprint)
        .unwrap();
    assert!(receipt.pending_roots.is_empty());
    assert_eq!(receipt.configuration.roots.len(), 2);
    assert_eq!(
        receipt.configuration.roots[0].path,
        receipt.configuration.roots[1].path
    );
    assert_ne!(
        receipt.configuration.roots[0].id,
        receipt.configuration.roots[1].id
    );
    assert_ne!(
        receipt.configuration.roots[0].space,
        receipt.configuration.roots[1].space
    );
}

#[test]
fn tcm_directory_rows_survive_import_restart_save_and_offline_recovery() {
    use tcm_core::folders::{FolderKind, FolderSource};
    let (_temp, old, store) = setup();
    let root = old.parent().unwrap();
    let movie = root.join("movies");
    let offline = root.join("offline-tv");
    let disabled = root.join("disabled");
    let mixed = root.join("mixed");
    let auto = root.join("auto");
    for path in [&movie, &mixed, &auto] {
        fs::create_dir(path).unwrap();
    }
    let value = serde_json::json!({"roots_configured":true,"library_roots":[
        {"path":movie,"name":"自定义电影名","kind":"movies","source":"auto","enabled":true},
        {"path":offline,"name":"离线剧集","kind":"tv","source":"manual","enabled":true},
        {"path":disabled,"name":"保留但停用","kind":"mixed","source":"auto","enabled":false},
        {"path":mixed,"name":"混合目录","kind":"mixed","source":"manual","enabled":true},
        {"path":auto,"name":"自动分类","kind":"auto","source":"auto","enabled":true}
    ]});
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend(serde_json::to_vec(&value).unwrap());
    fs::write(old.join("settings.json"), &bytes).unwrap();
    let modified = fs::metadata(old.join("settings.json"))
        .unwrap()
        .modified()
        .unwrap();
    let plan = store
        .prepare_migration("rows", &old, "tcm-portable")
        .unwrap();
    assert!(!plan
        .warnings
        .iter()
        .any(|warning| warning.starts_with("Malformed JSON")));
    let receipt = store.apply_migration("rows", &plan.fingerprint).unwrap();
    let folders = store.folder_settings().unwrap();
    assert_eq!(folders.folders.len(), 5);
    assert_eq!(
        folders
            .folders
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        [
            "自定义电影名",
            "离线剧集",
            "保留但停用",
            "混合目录",
            "自动分类"
        ]
    );
    assert_eq!(folders.folders[0].source, FolderSource::Auto);
    assert_eq!(folders.folders[1].kind, FolderKind::Tv);
    assert_eq!(folders.folders[2].kind, FolderKind::Mixed);
    assert!(!folders.folders[2].enabled);
    assert_eq!(folders.folders[4].kind, FolderKind::Auto);
    assert_eq!(receipt.configuration.roots.len(), 6);
    assert!(!receipt
        .pending_roots
        .iter()
        .any(|r| r.path == offline.to_str().unwrap()));
    assert!(store.catalog_summary().unwrap().roots_configured);
    assert!(store.tasks().unwrap().is_empty());
    assert_eq!(
        store.legacy_artifact("rows", "settings.json").unwrap(),
        bytes
    );
    drop(store);
    let store = Store::open(&root.join("new.sqlite")).unwrap();
    assert_eq!(store.folder_settings().unwrap(), folders);
    let saved = store
        .save_folder_settings("save-after-upgrade", folders)
        .unwrap();
    fs::create_dir(&offline).unwrap();
    let nfo = offline.join("tvshow.nfo");
    let raw = b"\xef\xbb\xbf<tvshow>\r\n<title>Restored</title></tvshow>\r\n";
    fs::write(&nfo, raw).unwrap();
    let nfo_time = fs::metadata(&nfo).unwrap().modified().unwrap();
    store
        .submit(ScanRequest {
            operation_id: "online".into(),
            space: Space::Tv,
            root_ids: saved
                .configuration
                .roots
                .iter()
                .filter(|r| r.path == offline.to_str().unwrap())
                .map(|r| r.id.clone())
                .collect(),
        })
        .unwrap();
    store.run_next(|| false, |_| {}).unwrap();
    assert_eq!(
        store
            .query(CatalogQuery {
                space: Space::Tv,
                search: String::new(),
                only_errors: false,
                offset: 0,
                limit: 100
            })
            .unwrap()
            .total,
        1
    );
    assert_eq!(fs::read(nfo.clone()).unwrap(), raw);
    assert_eq!(fs::metadata(nfo).unwrap().modified().unwrap(), nfo_time);
    assert_eq!(fs::read(old.join("settings.json")).unwrap(), bytes);
    assert_eq!(
        fs::metadata(old.join("settings.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
}

#[test]
fn tcm_import_preserves_existing_folder_choices_and_does_not_activate_unconfirmed_roots() {
    use tcm_core::folders::*;
    let (_temp, old, store) = setup();
    let media = old.parent().unwrap().join("media");
    fs::create_dir(&media).unwrap();
    let original = store
        .save_folder_settings(
            "current",
            FolderSettings {
                revision: 0,
                folders: vec![MediaFolder {
                    id: "existing".into(),
                    path: media.to_string_lossy().into(),
                    name: "当前名字".into(),
                    kind: FolderKind::Movies,
                    source: FolderSource::Manual,
                    enabled: true,
                }],
            },
        )
        .unwrap();
    for (id, configured) in [("unconfirmed", false), ("confirmed", true)] {
        fs::write(old.join("settings.json"),serde_json::to_vec(&serde_json::json!({"roots_configured":configured,"library_roots":[{"path":media,"name":"旧名字","kind":"mixed","source":"auto","enabled":true}]})).unwrap()).unwrap();
        let plan = store.prepare_migration(id, &old, "tcm-portable").unwrap();
        store.apply_migration(id, &plan.fingerprint).unwrap();
        assert_eq!(
            store.folder_settings().unwrap().folders,
            original.settings.folders
        );
        assert_eq!(
            store.configuration().unwrap().roots,
            original.configuration.roots
        );
    }
}

#[test]
fn tcm_folder_adapter_failure_rolls_back_files_configuration_and_interval() {
    let (_temp, old, store) = setup();
    let media = old.parent().unwrap().join("media");
    fs::create_dir(&media).unwrap();
    fs::write(old.join("settings.json"),serde_json::to_vec(&serde_json::json!({"interval_seconds":300,"roots_configured":true,"library_roots":[{"path":media,"name":"invalid\nname","kind":"movies","source":"manual","enabled":true}]})).unwrap()).unwrap();
    let plan = store
        .prepare_migration("bad-rows", &old, "tcm-portable")
        .unwrap();
    assert_eq!(
        store
            .apply_migration("bad-rows", &plan.fingerprint)
            .unwrap_err()
            .code,
        "migration-folders-invalid"
    );
    assert_eq!(store.configuration().unwrap().revision, 0);
    assert!(store.folder_settings().unwrap().folders.is_empty());
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 60);
    assert!(store.legacy_artifact("bad-rows", "settings.json").is_err());
}

#[test]
fn tcm_unusable_root_is_pending_and_an_unconfirmed_source_stays_unconfigured() {
    let (_temp, old, store) = setup();
    let path = old.parent().unwrap().join("file-instead-of-directory");
    fs::write(&path, b"not media").unwrap();
    for (id, configured) in [("draft", false), ("unusable", true)] {
        fs::write(old.join("settings.json"),serde_json::to_vec(&serde_json::json!({"roots_configured":configured,"library_roots":[{"path":path,"kind":"movies","source":"manual","enabled":true}]})).unwrap()).unwrap();
        let plan = store.prepare_migration(id, &old, "tcm-portable").unwrap();
        let receipt = store.apply_migration(id, &plan.fingerprint).unwrap();
        assert!(receipt.configuration.roots.is_empty());
        assert!(!receipt.pending_roots.is_empty());
        if !configured {
            assert_eq!(receipt.pending_roots[0].state, "not-configured");
            assert!(!store.catalog_summary().unwrap().roots_configured);
        } else {
            assert_eq!(
                receipt.pending_roots[0].state,
                "unavailable-or-needs-mapping"
            );
        }
        assert!(store.tasks().unwrap().is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"not media");
    }
}
