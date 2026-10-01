use tcm_core::{legacy_process::ExecutableEvidence, legacy_startup::RunBackup, store::Store};
fn backup() -> RunBackup {
    RunBackup {
        name: "Tech Card Manager".into(),
        command: r#""C:\电影\Tech Card Manager.exe" --login-startup"#
            .encode_utf16()
            .collect(),
        executable: ExecutableEvidence {
            path: r"C:\电影\Tech Card Manager.exe".into(),
            sha256: "a".repeat(64),
            baseline_assets_match: true,
        },
    }
}
#[test]
fn run_backup_is_immutable_lossless_and_readable_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("store.sqlite");
    let store = Store::open(&database).unwrap();
    let original = backup();
    let id = store.backup_legacy_run(&original).unwrap();
    assert_eq!(store.backup_legacy_run(&original).unwrap(), id);
    drop(store);
    let store = Store::open(&database).unwrap();
    let restored = store.legacy_run_backup(&id).unwrap();
    assert_eq!(restored.command, original.command);
    assert_eq!(restored.path().unwrap(), original.path().unwrap());
    let mut changed = original.clone();
    changed.executable.sha256 = "b".repeat(64);
    let second = store.backup_legacy_run(&changed).unwrap();
    assert_ne!(id, second);
    assert_eq!(
        store.legacy_run_backup(&id).unwrap().executable.sha256,
        original.executable.sha256
    );
    assert!(store.legacy_run_backup("../invalid").is_err());
    assert!(store.legacy_run_backup(&"f".repeat(64)).is_err());
}
#[test]
fn corrupt_backups_fail_closed_and_are_not_replaced_with_apparent_success() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("store.sqlite")).unwrap();
    let original = backup();
    let id = store.backup_legacy_run(&original).unwrap();
    let key = format!("legacy-run-backup:{id}");
    let changed = serde_json::json!({"unexpected":"foreign record"});
    store.save_preference(&key, &changed).unwrap();
    assert_eq!(
        store.legacy_run_backup(&id).unwrap_err().code,
        "legacy-startup-backup-corrupt"
    );
    assert_eq!(
        store.backup_legacy_run(&original).unwrap_err().code,
        "legacy-startup-backup-conflict"
    );
    assert_eq!(store.preferences(&key).unwrap(), changed);
    let mut unsupported = original;
    unsupported.command =
        r#""C:\old.exe" --login-startup --secret private"#.encode_utf16().collect();
    assert_eq!(
        store.backup_legacy_run(&unsupported).unwrap_err().code,
        "legacy-startup-unowned"
    );
}

#[test]
fn interrupted_and_absent_removals_cannot_authorize_restoration_after_restart() {
    use tcm_core::legacy_startup::RunPhase;
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("store.sqlite");
    let store = Store::open(&database).unwrap();
    let backup = store.backup_legacy_run(&backup()).unwrap();
    let interrupted = store
        .begin_legacy_run_removal("interrupted", &backup)
        .unwrap();
    let absent = store.begin_legacy_run_removal("absent", &backup).unwrap();
    let absent = store.advance_legacy_run(&absent, RunPhase::Absent).unwrap();
    drop(store);
    let store = Store::open(&database).unwrap();
    assert_eq!(
        store.legacy_run_receipt("interrupted").unwrap(),
        interrupted
    );
    assert_eq!(store.legacy_run_receipt("absent").unwrap(), absent);
    for receipt in [interrupted, absent] {
        assert_eq!(
            store
                .advance_legacy_run(&receipt, RunPhase::Restoring)
                .unwrap_err()
                .code,
            "legacy-startup-operation-phase"
        );
        assert_eq!(
            store
                .begin_legacy_run_removal(&receipt.id, &backup)
                .unwrap_err()
                .code,
            "legacy-startup-operation-exists"
        );
        assert_eq!(store.legacy_run_receipt(&receipt.id).unwrap(), receipt);
    }
}

#[test]
fn confirmed_removal_and_restore_receipts_survive_restart_without_stale_regression() {
    use tcm_core::legacy_startup::RunPhase;
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("store.sqlite");
    let store = Store::open(&database).unwrap();
    let original = backup();
    let fingerprint = store.backup_legacy_run(&original).unwrap();
    let claimed = store
        .begin_legacy_run_removal("rollback", &fingerprint)
        .unwrap();
    let removed = store
        .advance_legacy_run(&claimed, RunPhase::Removed)
        .unwrap();
    let restoring = store
        .advance_legacy_run(&removed, RunPhase::Restoring)
        .unwrap();
    drop(store);
    let store = Store::open(&database).unwrap();
    assert_eq!(store.legacy_run_receipt("rollback").unwrap(), restoring);
    assert_eq!(
        store.legacy_run_backup(&restoring.backup).unwrap().command,
        original.command
    );
    // A late result from deletion cannot turn a restore in progress into absent.
    assert_eq!(
        store
            .advance_legacy_run(&claimed, RunPhase::Absent)
            .unwrap_err()
            .code,
        "legacy-startup-operation-conflict"
    );
    let restored = store
        .advance_legacy_run(&restoring, RunPhase::Restored)
        .unwrap();
    assert_eq!(
        store
            .advance_legacy_run(&removed, RunPhase::Restoring)
            .unwrap_err()
            .code,
        "legacy-startup-operation-conflict"
    );
    assert!(store
        .advance_legacy_run(&restored, RunPhase::Removing)
        .is_err());
    drop(store);
    let store = Store::open(&database).unwrap();
    assert_eq!(store.legacy_run_receipt("rollback").unwrap(), restored);
    assert!(store.legacy_run_receipt("../rollback").is_err());
    assert!(store.legacy_run_receipt("not-present").is_err());
}

#[test]
fn simultaneous_removal_requests_claim_one_operation_and_keep_its_backup_binding() {
    let temp = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(Store::open(&temp.path().join("store.sqlite")).unwrap());
    let original = backup();
    let fingerprint = store.backup_legacy_run(&original).unwrap();
    let mut changed = original.clone();
    changed.executable.sha256 = "b".repeat(64);
    let other = store.backup_legacy_run(&changed).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = [fingerprint.clone(), other]
        .into_iter()
        .map(|backup| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.begin_legacy_run_removal("one-confirmation", &backup)
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let winner = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap();
    assert_eq!(
        &store.legacy_run_receipt("one-confirmation").unwrap(),
        winner
    );
    assert_eq!(
        results
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .code,
        "legacy-startup-operation-exists"
    );
    assert_eq!(
        store.legacy_run_backup(&fingerprint).unwrap().command,
        original.command
    );
}
