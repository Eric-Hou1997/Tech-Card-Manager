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
