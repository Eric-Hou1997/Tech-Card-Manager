use tcm_core::{
    hash,
    legacy_agent::{Identity, Snapshot},
    legacy_components::{Component, Inventory},
    legacy_migration::{self as migration, Phase, Platform, StepPhase},
    legacy_process::ExecutableEvidence,
    legacy_startup::{Removal, RunBackup, RunPhase},
    store::Store,
    AppError, Result,
};
#[derive(Default)]
struct Host {
    inventory: Inventory,
    backups: Vec<RunBackup>,
    calls: Vec<String>,
    fail_run: Option<String>,
    uncertain: bool,
    crash_run: bool,
    inspections: usize,
    change_at: Option<usize>,
    agent_snapshots: Vec<Snapshot>,
    archives: std::collections::BTreeMap<String, Snapshot>,
    crash_agent: bool,
    fail_agent: Option<String>,
    vanish_agent_on_stop: bool,
}
fn run(name: &str) -> (Component, RunBackup) {
    let mode = if name == "Tech Card Manager" {
        "--login-startup"
    } else {
        "--agent"
    };
    let backup = RunBackup {
        name: name.into(),
        command: format!("\"C:\\原版\\Manager.exe\" {mode}")
            .encode_utf16()
            .collect(),
        executable: ExecutableEvidence {
            path: "C:\\原版\\Manager.exe".into(),
            sha256: "a".repeat(64),
            baseline_assets_match: true,
        },
    };
    let command = hash(
        &backup
            .command
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    (
        Component {
            kind: "startup".into(),
            target: name.into(),
            identity: hash(
                &serde_json::to_vec(&(Some(command), Some(&backup.executable))).unwrap(),
            ),
            pid: None,
        },
        backup,
    )
}
fn host() -> Host {
    let (login, backup) = run("Tech Card Manager");
    Host {
        inventory: Inventory {
            items: vec![
                login,
                Component {
                    kind: "process".into(),
                    target: "C:\\原版\\Manager.exe".into(),
                    identity: "process-evidence".into(),
                    pid: Some(123),
                },
            ],
            errors: vec![],
        },
        backups: vec![backup],
        ..Default::default()
    }
}
fn add_agent(host: &mut Host, root: &std::path::Path, name: &str) -> Snapshot {
    let value = Snapshot {
        path: root.join("data").join(name).to_str().unwrap().into(),
        identity: Identity {
            file: name.into(),
            created: "created".into(),
            written: "written".into(),
            len: 3,
            attributes: 0,
        },
        contents: b"123".to_vec(),
    };
    host.inventory.items.push(agent_component(&value));
    host.agent_snapshots.push(value.clone());
    value
}
fn agent_component(value: &Snapshot) -> Component {
    Component {
        kind: "agent-file".into(),
        target: value.path.clone(),
        identity: value.fingerprint().unwrap(),
        pid: None,
    }
}
impl Platform for Host {
    fn prepare_agent(&mut self, item: &Component) -> Result<Snapshot> {
        self.agent_snapshots
            .iter()
            .find(|v| v.path == item.target)
            .cloned()
            .ok_or_else(|| AppError::new("legacy-system-component-unverified", "no owned snapshot"))
    }
    fn archive_agent(&mut self, value: &Snapshot, operation: &str) -> Result<()> {
        if self.archives.get(operation) == Some(value) {
            return Ok(());
        }
        if self.fail_agent.as_deref() == Some(&value.path) {
            self.inventory
                .items
                .iter_mut()
                .find(|v| v.target == value.path)
                .unwrap()
                .identity = "foreign".into();
            return Err(AppError::new("archive-conflict", "replacement fixture"));
        }
        let item = agent_component(value);
        if !self.inventory.items.contains(&item) {
            return Err(AppError::new("archive-unverified", "not the original file"));
        }
        self.calls.push(format!("archive:{}", value.path));
        self.inventory.items.retain(|v| v != &item);
        self.archives.insert(operation.into(), value.clone());
        if self.crash_agent {
            self.crash_agent = false;
            panic!("interrupted after file rename");
        }
        Ok(())
    }
    fn restore_agent(&mut self, value: &Snapshot, operation: &str) -> Result<()> {
        let item = agent_component(value);
        if let Some(current) = self.inventory.items.iter().find(|v| v.target == value.path) {
            if current == &item && !self.archives.contains_key(operation) {
                return Ok(());
            }
            return Err(AppError::new("restore-conflict", "preserve replacement"));
        }
        if self.archives.get(operation) != Some(value) {
            return Err(AppError::new("restore-unverified", "missing original"));
        }
        self.calls.push(format!("restore-file:{}", value.path));
        self.archives.remove(operation);
        self.inventory.items.push(item);
        Ok(())
    }
    fn inspect(&mut self) -> Result<Inventory> {
        self.inspections += 1;
        if self.change_at == Some(self.inspections) {
            self.inventory.items[0].identity = "replaced".into();
        }
        Ok(self.inventory.clone())
    }
    fn prepare_run(&mut self, item: &Component) -> Result<RunBackup> {
        Ok(self
            .backups
            .iter()
            .find(|b| b.name == item.target)
            .unwrap()
            .clone())
    }
    fn stop_process(&mut self, item: &Component) -> Result<()> {
        self.calls.push("stop".into());
        if self.vanish_agent_on_stop {
            self.inventory
                .items
                .retain(|item| item.kind != "agent-file");
        }
        self.inventory.items.retain(|other| other != item);
        Ok(())
    }
    fn remove_run(&mut self, store: &Store, id: &str, backup: &str) -> Result<Removal> {
        match store.legacy_run_receipt(id) {
            Ok(receipt) => {
                return match receipt.phase {
                    RunPhase::Removed => Ok(Removal::Removed),
                    RunPhase::Absent => Ok(Removal::AlreadyAbsent),
                    _ => Err(AppError::new("unverified", "unknown removal")),
                }
            }
            Err(error) if error.code == "legacy-startup-operation-missing" => {}
            Err(error) => return Err(error),
        }
        let value = store.legacy_run_backup(backup)?;
        let receipt = store.begin_legacy_run_removal(id, backup)?;
        self.calls.push(format!("remove:{}", value.name));
        if self.fail_run.as_ref() == Some(&value.name) {
            if !self.uncertain {
                store.advance_legacy_run(&receipt, RunPhase::Absent)?;
            }
            return Err(AppError::new("delete-denied", "fixture deletion failed"));
        }
        self.inventory
            .items
            .retain(|item| item.target != value.name);
        store.advance_legacy_run(&receipt, RunPhase::Removed)?;
        if self.crash_run {
            self.crash_run = false;
            panic!("simulated process interruption after Run receipt");
        }
        Ok(Removal::Removed)
    }
    fn restore_run(&mut self, store: &Store, id: &str) -> Result<()> {
        let receipt = store.legacy_run_receipt(id)?;
        let backup = store.legacy_run_backup(&receipt.backup)?;
        self.calls.push(format!("restore:{}", backup.name));
        let receipt = store.advance_legacy_run(&receipt, RunPhase::Restoring)?;
        self.inventory.items.push(run(&backup.name).0);
        store.advance_legacy_run(&receipt, RunPhase::Restored)?;
        Ok(())
    }
}
#[test]
fn complete_system_receipt_replays_after_restart_without_repeating_effects() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite");
    let store = Store::open(&db).unwrap();
    let mut host = host();
    let reviewed = host.inventory.fingerprint().unwrap();
    let plan = migration::execute(&store, &mut host, "confirmed", &reviewed).unwrap();
    assert_eq!(plan.phase, Phase::Complete);
    assert_eq!(host.calls, ["stop", "remove:Tech Card Manager"]);
    assert!(host.inventory.items.is_empty());
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "confirmed", &reviewed)
            .unwrap()
            .phase,
        Phase::Complete
    );
    assert_eq!(host.calls.len(), 2);
    assert!(migration::execute(&store, &mut host, "confirmed", "different").is_err());
    host.inventory.items.push(Component {
        kind: "task".into(),
        target: "new task".into(),
        identity: "new".into(),
        pid: None,
    });
    assert!(migration::execute(&store, &mut host, "confirmed", &reviewed).is_err());
    assert_eq!(host.calls.len(), 2);
}
#[test]
fn unsupported_unknown_and_changed_inventory_cannot_begin_cleanup() {
    for mode in ["task", "agent-file", "error", "changed"] {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        let mut host = host();
        match mode {
            "error" => host
                .inventory
                .errors
                .push(AppError::new("read-denied", "unknown")),
            "changed" => host.change_at = Some(2),
            kind => host.inventory.items.push(Component {
                kind: kind.into(),
                target: "unverified target".into(),
                identity: "unknown".into(),
                pid: None,
            }),
        }
        let reviewed = host.inventory.fingerprint().unwrap();
        assert!(migration::execute(&store, &mut host, "confirmed", &reviewed).is_err());
        assert!(host.calls.is_empty());
        assert!(store.legacy_system_plan("confirmed").unwrap().is_none());
    }
}
#[test]
fn damaged_prepared_backup_blocks_process_shutdown_before_any_effect() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    let mut host = host();
    let reviewed = host.inventory.fingerprint().unwrap();
    let plan = migration::prepare(&store, &mut host, "confirmed", &reviewed).unwrap();
    let backup = plan.steps[0].backup.as_ref().unwrap();
    store
        .save_preference(
            &format!("legacy-run-backup:{backup}"),
            &serde_json::json!({"damaged":true}),
        )
        .unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "confirmed", &reviewed)
            .unwrap_err()
            .code,
        "legacy-startup-backup-corrupt"
    );
    assert!(host.calls.is_empty());
    assert_eq!(
        store.latest_legacy_system_plan().unwrap().unwrap().phase,
        Phase::Planned
    );
}
#[test]
fn later_failure_restores_only_proven_run_removals_and_does_not_relaunch_processes() {
    for uncertain in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        let mut host = host();
        let (entry, backup) = run("IMDb Tech Manager Agent");
        host.inventory.items.push(entry);
        host.backups.push(backup);
        host.fail_run = Some("IMDb Tech Manager Agent".into());
        host.uncertain = uncertain;
        let reviewed = host.inventory.fingerprint().unwrap();
        assert_eq!(
            migration::execute(&store, &mut host, "confirmed", &reviewed)
                .unwrap_err()
                .code,
            "delete-denied"
        );
        let plan = store.legacy_system_plan("confirmed").unwrap().unwrap();
        assert_eq!(plan.phase, Phase::Failed);
        assert_eq!(plan.steps[0].phase, StepPhase::Restored);
        assert_eq!(
            host.calls,
            [
                "stop",
                "remove:Tech Card Manager",
                "remove:IMDb Tech Manager Agent",
                "restore:Tech Card Manager"
            ]
        );
        assert_eq!(plan.recovery_errors.len(), usize::from(uncertain));
        assert!(!host
            .inventory
            .items
            .iter()
            .any(|item| item.kind == "process"));
        assert!(migration::execute(&store, &mut host, "confirmed", &reviewed).is_err());
        assert_eq!(host.calls.len(), 4);
    }
}
#[test]
fn interruption_between_run_and_system_receipts_recovers_the_same_step_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite");
    let store = Store::open(&db).unwrap();
    let mut host = host();
    host.crash_run = true;
    let reviewed = host.inventory.fingerprint().unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| migration::execute(
            &store,
            &mut host,
            "confirmed",
            &reviewed
        )))
        .is_err()
    );
    let plan = store.legacy_system_plan("confirmed").unwrap().unwrap();
    assert_eq!(plan.phase, Phase::Running);
    assert_eq!(plan.steps[0].phase, StepPhase::Working);
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(
        store.latest_legacy_system_plan().unwrap().unwrap().id,
        "confirmed"
    );
    assert_eq!(
        store.preferences("legacy-system-last").unwrap(),
        serde_json::json!("confirmed")
    );
    let fresh_review = host.inventory.fingerprint().unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "other-confirmation", &fresh_review)
            .unwrap_err()
            .code,
        "legacy-system-recovery-required"
    );
    assert_eq!(
        migration::execute(&store, &mut host, "confirmed", &reviewed)
            .unwrap()
            .phase,
        Phase::Complete
    );
    assert_eq!(host.calls, ["stop", "remove:Tech Card Manager"]);
}

#[test]
fn agent_rename_interruption_resumes_the_same_step_without_repeating_effects() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite");
    let store = Store::open(&db).unwrap();
    let mut host = host();
    let file = add_agent(&mut host, temp.path(), "agent.pid");
    host.crash_agent = true;
    let reviewed = host.inventory.fingerprint().unwrap();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        migration::execute(&store, &mut host, "agent-recovery", &reviewed)
    }));
    assert!(interrupted.is_err());
    let plan = store.latest_legacy_system_plan().unwrap().unwrap();
    assert_eq!(plan.phase, Phase::Running);
    assert_eq!(plan.steps[2].phase, StepPhase::Working);
    assert_eq!(
        store
            .legacy_agent_backup(plan.steps[2].backup.as_ref().unwrap())
            .unwrap(),
        file
    );
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "agent-recovery", &reviewed)
            .unwrap()
            .phase,
        Phase::Complete
    );
    assert_eq!(
        host.calls,
        [
            "stop".to_string(),
            "remove:Tech Card Manager".to_string(),
            format!("archive:{}", file.path)
        ]
    );
    assert_eq!(
        migration::execute(&store, &mut host, "agent-recovery", &reviewed)
            .unwrap()
            .phase,
        Phase::Complete
    );
    assert_eq!(host.calls.len(), 3);
}
#[test]
fn later_agent_conflict_restores_prior_file_and_run_but_preserves_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    let mut host = host();
    let first = add_agent(&mut host, temp.path(), "agent.pid");
    let second = add_agent(&mut host, temp.path(), "agent-heartbeat.txt");
    host.fail_agent = Some(second.path.clone());
    let reviewed = host.inventory.fingerprint().unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "agent-conflict", &reviewed)
            .unwrap_err()
            .code,
        "archive-conflict"
    );
    let plan = store.latest_legacy_system_plan().unwrap().unwrap();
    assert_eq!(plan.phase, Phase::Failed);
    assert_eq!(plan.steps[0].phase, StepPhase::Restored);
    assert_eq!(plan.steps[1].phase, StepPhase::Done);
    assert_eq!(plan.steps[2].phase, StepPhase::Restored);
    assert_eq!(plan.recovery_errors.len(), 1);
    assert_eq!(plan.recovery_errors[0].code, "restore-conflict");
    assert!(host.inventory.items.contains(&agent_component(&first)));
    assert_eq!(
        host.inventory
            .items
            .iter()
            .find(|v| v.target == second.path)
            .unwrap()
            .identity,
        "foreign"
    );
    assert!(host.archives.is_empty());
    assert_eq!(host.calls.iter().filter(|call| *call == "stop").count(), 1);
}
#[test]
fn damaged_agent_backup_blocks_stop_and_independently_removed_files_are_not_restored() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
    let mut host = host();
    add_agent(&mut host, temp.path(), "agent.pid");
    let reviewed = host.inventory.fingerprint().unwrap();
    let plan = migration::prepare(&store, &mut host, "damaged-agent", &reviewed).unwrap();
    let backup = plan.steps[2].backup.as_ref().unwrap();
    store
        .save_preference(
            &format!("legacy-agent-backup:{backup}"),
            &serde_json::json!({"corrupt":true}),
        )
        .unwrap();
    assert_eq!(
        migration::execute(&store, &mut host, "damaged-agent", &reviewed)
            .unwrap_err()
            .code,
        "legacy-agent-backup-corrupt"
    );
    assert!(host.calls.is_empty());
    let store = Store::open(&temp.path().join("independent.sqlite")).unwrap();
    host.vanish_agent_on_stop = true;
    let plan = migration::execute(&store, &mut host, "independent", &reviewed).unwrap();
    assert_eq!(plan.steps[2].phase, StepPhase::Absent);
    assert!(host.archives.is_empty());
    assert_eq!(host.calls, ["stop", "remove:Tech Card Manager"]);
}

#[test]
fn reviewed_agent_directory_remains_known_after_its_last_discoverable_item_is_gone() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.sqlite");
    let store = Store::open(&db).unwrap();
    let mut host = Host::default();
    add_agent(&mut host, temp.path(), "agent.pid");
    let reviewed = host.inventory.fingerprint().unwrap();
    migration::execute(&store, &mut host, "known-root", &reviewed).unwrap();
    assert!(host.inventory.items.is_empty());
    drop(store);
    let store = Store::open(&db).unwrap();
    let plan = store.latest_legacy_system_plan().unwrap().unwrap();
    assert_eq!(
        tcm_core::legacy_components::reviewed_roots(&store, &plan).unwrap(),
        [temp.path()]
    );
}
