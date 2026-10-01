use std::fs;
use tcm_core::{store::Store, *};

fn setup() -> (tempfile::TempDir, std::path::PathBuf, Store) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let portable = root.join("portable");
    fs::create_dir_all(portable.join("data")).unwrap();
    fs::create_dir_all(portable.join("runtime/engine")).unwrap();
    fs::create_dir(portable.join("logs")).unwrap();
    fs::write(
        portable.join("runtime/engine/windows-engine.ps1"),
        include_bytes!("../../../../windows/engine/windows-engine.ps1"),
    )
    .unwrap();
    fs::write(
        portable.join("data/settings.json"),
        br#"{"roots_configured":false,"interval_seconds":300,"language":"en-US"}"#,
    )
    .unwrap();
    fs::write(portable.join("logs/job.log"), b"\xef\xbb\xbfOriginal\r\n").unwrap();
    let store = Store::open(&root.join("new.sqlite")).unwrap();
    (temp, portable, store)
}

#[test]
fn another_install_directory_can_restore_from_the_original_login_location() {
    let (temp, portable, store) = setup();
    assert!(store.startup_import_receipt().unwrap().is_none());
    let installed = temp.path().canonicalize().unwrap().join("new-install");
    fs::create_dir(&installed).unwrap();
    let settings = portable.join("data/settings.json");
    let before = fs::read(&settings).unwrap();
    let modified = fs::metadata(&settings).unwrap().modified().unwrap();
    let receipt = store
        .import_portable_from_startup_sources(&installed, || Ok(Some(portable.clone())))
        .unwrap()
        .unwrap();
    assert_eq!(store.configuration().unwrap().locale, Locale::English);
    assert_eq!(
        store.manager_job().unwrap().unwrap()["log"],
        "\u{feff}Original\r\n"
    );
    assert_eq!(fs::read(&settings).unwrap(), before);
    assert_eq!(
        fs::metadata(&settings).unwrap().modified().unwrap(),
        modified
    );
    fs::rename(&portable, portable.with_file_name("moved")).unwrap();
    drop(store);
    let store = Store::open(&temp.path().join("new.sqlite")).unwrap();
    // A stopped old installation with its login entry removed is still a known
    // source. The getter must not access it, reimport it, or assert ownership.
    let recorded = store.startup_import_receipt().unwrap().unwrap();
    assert_eq!(recorded.source, portable.to_string_lossy());
    assert_eq!(recorded.id, receipt.id);
    assert!(!portable.exists());
    assert_eq!(
        store
            .import_portable_from_startup_sources(&installed, || panic!(
                "committed import must not reread the login entry"
            ))
            .unwrap()
            .unwrap()
            .id,
        receipt.id
    );
}

#[test]
fn adjacent_or_existing_data_wins_and_failed_locators_remain_retryable() {
    let (temp, portable, store) = setup();
    assert!(store
        .import_portable_from_startup_sources(&portable, || panic!("adjacent source wins"))
        .unwrap()
        .is_some());
    let initialized = Store::open(&temp.path().join("used.sqlite")).unwrap();
    initialized
        .configure("settings", Configuration::default())
        .unwrap();
    assert!(initialized
        .import_portable_from_startup_sources(&temp.path().join("absent"), || panic!(
            "used destination must not consult login entry"
        ))
        .unwrap()
        .is_none());
    let blank = Store::open(&temp.path().join("blank.sqlite")).unwrap();
    let absent = temp.path().join("absent");
    assert_eq!(
        blank
            .import_portable_from_startup_sources(&absent, || Err(AppError::new(
                "migration-startup-read",
                "denied"
            )))
            .unwrap_err()
            .code,
        "migration-startup-read"
    );
    assert_eq!(blank.configuration().unwrap().revision, 0);
    assert!(blank
        .import_portable_from_startup_sources(&absent, || Ok(Some(portable)))
        .unwrap()
        .is_some());
}

#[test]
fn startup_import_is_readonly_and_receipt_prevents_reimport_after_edits_or_source_removal() {
    let (_temp, portable, store) = setup();
    let path = portable.join("data/settings.json");
    let bytes = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let log_modified: i64 = fs::metadata(portable.join("logs/job.log"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
    let receipt = store.import_portable_on_start(&portable).unwrap().unwrap();
    assert_eq!(store.configuration().unwrap().locale, Locale::English);
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 300);
    assert_eq!(
        store.legacy_artifact(&receipt.id, "logs/job.log").unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
    assert_eq!(store.diagnostic_logs().unwrap()[0].2, Some(log_modified));
    assert!(store.tasks().unwrap().is_empty());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    let mut config = store.configuration().unwrap();
    config.locale = Locale::French;
    store.configure("new-choice", config).unwrap();
    fs::write(&path, b"{invalid").unwrap();
    assert_eq!(
        store
            .import_portable_on_start(&portable)
            .unwrap()
            .unwrap()
            .id,
        receipt.id
    );
    assert_eq!(store.configuration().unwrap().locale, Locale::French);
    let database = portable.parent().unwrap().join("new.sqlite");
    fs::rename(&portable, portable.with_file_name("offline")).unwrap();
    drop(store);
    let store = Store::open(&database).unwrap();
    assert_eq!(
        store
            .import_portable_on_start(&portable)
            .unwrap()
            .unwrap()
            .id,
        receipt.id
    );
    assert_eq!(store.configuration().unwrap().locale, Locale::French);
}

#[test]
fn schema_six_archives_upgrade_without_inventing_missing_source_times() {
    let (temp, portable, store) = setup();
    let receipt = store.import_portable_on_start(&portable).unwrap().unwrap();
    let original = store.legacy_artifact(&receipt.id, "logs/job.log").unwrap();
    drop(store);
    let path = temp.path().join("new.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("ALTER TABLE legacy_artifacts DROP COLUMN modified_unix", [])
        .unwrap();
    db.pragma_update(None, "user_version", 6).unwrap();
    drop(db);

    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.legacy_artifact(&receipt.id, "logs/job.log").unwrap(),
        original
    );
    assert_eq!(store.diagnostic_logs().unwrap()[0].2, None);
    drop(store);
    let db = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        7
    );
}

#[test]
fn changed_incomplete_attempt_can_retry_without_partial_preferences_or_archives() {
    let (_temp, portable, store) = setup();
    let media = portable.parent().unwrap().join("media");
    fs::create_dir(&media).unwrap();
    let mut settings = serde_json::json!({"roots_configured":true,"interval_seconds":300,"library_roots":[{"path":media,"name":"bad\nname","kind":"movies","source":"manual","enabled":true}]});
    fs::write(
        portable.join("data/settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.import_portable_on_start(&portable).unwrap_err().code,
        "migration-folders-invalid"
    );
    assert_eq!(store.configuration().unwrap().revision, 0);
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 60);
    assert_eq!(store.history_archives(0, 100).unwrap().total, 0);
    settings["library_roots"][0]["name"] = serde_json::json!("Recovered");
    fs::write(
        portable.join("data/settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let receipt = store.import_portable_on_start(&portable).unwrap().unwrap();
    assert_eq!(
        store.folder_settings().unwrap().folders[0].name,
        "Recovered"
    );
    assert_eq!(store.configuration().unwrap().revision, 1);
    assert!(receipt.pending_roots.is_empty());
}

#[test]
fn source_identity_settings_validity_and_existing_destination_are_checked_before_import() {
    let (_temp, portable, store) = setup();
    assert!(store
        .import_portable_on_start(&portable.join("absent"))
        .unwrap()
        .is_none());
    let engine = portable.join("runtime/engine/windows-engine.ps1");
    fs::write(&engine, b"unrelated script").unwrap();
    assert_eq!(
        store.import_portable_on_start(&portable).unwrap_err().code,
        "migration-source-unverified"
    );
    assert_eq!(store.configuration().unwrap().revision, 0);
    fs::write(
        &engine,
        include_bytes!("../../../../windows/engine/windows-engine.ps1"),
    )
    .unwrap();
    fs::write(portable.join("data/settings.json"), b"{invalid").unwrap();
    assert_eq!(
        store.import_portable_on_start(&portable).unwrap_err().code,
        "migration-settings-invalid"
    );
    store
        .configure("existing", Configuration::default())
        .unwrap();
    assert!(store.import_portable_on_start(&portable).unwrap().is_none());
    assert_eq!(store.configuration().unwrap().revision, 1);
}

#[cfg(unix)]
#[test]
fn startup_source_cannot_follow_settings_or_engine_symlinks() {
    let (_temp, portable, store) = setup();
    let path = portable.join("data/settings.json");
    let outside = portable.parent().unwrap().join("outside.json");
    fs::rename(&path, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert_eq!(
        store.import_portable_on_start(&portable).unwrap_err().code,
        "ambiguous-path"
    );
    assert_eq!(store.configuration().unwrap().revision, 0);
}

#[test]
fn existing_interval_choice_blocks_automatic_import_even_without_media_roots() {
    let (_temp, portable, store) = setup();
    store
        .save_incremental_settings(
            "new-choice",
            tcm_core::incremental::IncrementalSettings {
                revision: 0,
                interval_seconds: 900,
            },
        )
        .unwrap();
    fs::write(portable.join("data/settings.json"), b"{invalid").unwrap();
    assert!(store.import_portable_on_start(&portable).unwrap().is_none());
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 900);
    assert_eq!(store.configuration().unwrap().revision, 0);
}

#[test]
fn obsolete_rewrite_ui_state_does_not_block_original_product_data_import() {
    let (_temp, portable, store) = setup();
    let mut state = store.ui_state().unwrap();
    state.active_space = Space::Tv;
    state.tv.search = "旧 rewrite 草稿".into();
    store.save_ui_state("obsolete-view", state).unwrap();

    assert!(store.import_portable_on_start(&portable).unwrap().is_some());
    assert_eq!(store.configuration().unwrap().locale, Locale::English);
    assert_eq!(store.incremental_settings().unwrap().interval_seconds, 300);
    assert_eq!(store.ui_state().unwrap().active_space, Space::Tv);
}

#[test]
fn pending_native_settings_prevent_a_startup_retry_from_importing_old_choices() {
    let (_temp, portable, store) = setup();
    store
        .prepare_lifecycle(
            "pending-user-choice",
            tcm_core::lifecycle::Settings::default(),
            false,
        )
        .unwrap();
    assert!(store.import_portable_on_start(&portable).unwrap().is_none());
    assert_eq!(store.configuration().unwrap().revision, 0);
    assert_eq!(
        store.preferences("lifecycle-pending").unwrap(),
        serde_json::json!("pending-user-choice")
    );
    assert_eq!(store.history_archives(0, 100).unwrap().total, 0);
}
