use tcm_core::{card_service::ServiceStatus, store::Store};
fn status(at: Option<&str>, phase: &str) -> ServiceStatus {
    ServiceStatus {
        phase: phase.into(),
        last_started_at: at.map(str::to_owned),
        lease: None,
        error: None,
    }
}
#[test]
fn actual_start_survives_stop_restart_and_late_status_without_inventing_time() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite");
    let store = Store::open(&path).unwrap();
    assert!(store
        .service_status_with_history(status(None, "stopped"))
        .unwrap()
        .last_started_at
        .is_none());
    let first = "2026-09-12T01:02:03Z";
    let second = "2026-09-12T02:03:04Z";
    store
        .service_status_with_history(status(Some(first), "running"))
        .unwrap();
    assert_eq!(
        store
            .service_status_with_history(status(None, "stopped"))
            .unwrap()
            .last_started_at
            .as_deref(),
        Some(first)
    );
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .service_status_with_history(status(None, "stopped"))
            .unwrap()
            .last_started_at
            .as_deref(),
        Some(first)
    );
    store
        .service_status_with_history(status(Some(second), "running"))
        .unwrap();
    store
        .service_status_with_history(status(Some(first), "running"))
        .unwrap();
    assert_eq!(
        store
            .service_status_with_history(status(None, "stopped"))
            .unwrap()
            .last_started_at
            .as_deref(),
        Some(second)
    );
    assert!(store
        .service_status_with_history(status(Some("invalid"), "running"))
        .is_err());
    assert_eq!(store.preferences("emby-last-started-at").unwrap(), second);
}
#[test]
fn storage_failure_leaves_old_history_and_old_helper_reply_remains_readable() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite");
    let store = Store::open(&path).unwrap();
    let old: ServiceStatus =
        serde_json::from_str(r#"{"phase":"stopped","lease":null,"error":null}"#).unwrap();
    assert!(old.last_started_at.is_none());
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_start_time BEFORE INSERT ON preferences WHEN NEW.key='emby-last-started-at' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(store
        .service_status_with_history(status(Some("2026-09-12T01:02:03Z"), "running"))
        .is_err());
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM preferences WHERE key='emby-last-started-at'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
}
