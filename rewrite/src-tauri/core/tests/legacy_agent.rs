use std::{fs, path::PathBuf};
use tcm_core::{
    legacy_agent::{self as agent, Snapshot},
    store::Store,
};
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("runtime/engine")).unwrap();
    fs::write(
        root.join("runtime/engine/windows-engine.ps1"),
        include_bytes!("../../../../windows/engine/windows-engine.ps1"),
    )
    .unwrap();
    fs::create_dir(root.join("data")).unwrap();
    let file = root.join("data/agent.pid");
    fs::write(&file, b"\xef\xbb\xbf123\r\n").unwrap();
    (temp, file)
}
#[test]
fn snapshots_keep_original_bytes_and_detect_same_content_replacement() {
    let (_temp, file) = fixture();
    let first = agent::snapshot(&file).unwrap();
    assert_eq!(first.contents, b"\xef\xbb\xbf123\r\n");
    assert_eq!(agent::snapshot(&file).unwrap(), first);
    // Keep the first inode allocated; identical bytes are not the same object.
    fs::rename(&file, file.with_extension("original")).unwrap();
    fs::write(&file, &first.contents).unwrap();
    let second = agent::snapshot(&file).unwrap();
    assert_ne!(second.identity.file, first.identity.file);
    assert_ne!(second.fingerprint().unwrap(), first.fingerprint().unwrap());
    assert!(first.archive_path("../other").is_err());
    assert_eq!(
        first.archive_path(&"a".repeat(64)).unwrap().parent(),
        file.parent()
    );
}
#[test]
fn backup_survives_restart_and_corruption_never_overwrites_saved_evidence() {
    let (temp, file) = fixture();
    let db = temp.path().join("state.sqlite");
    let store = Store::open(&db).unwrap();
    let value = agent::snapshot(&file).unwrap();
    let id = store.backup_legacy_agent(&value).unwrap();
    assert_eq!(store.backup_legacy_agent(&value).unwrap(), id);
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.legacy_agent_backup(&id).unwrap(), value);
    let key = format!("legacy-agent-backup:{id}");
    let corrupt = serde_json::json!({"foreign":true});
    store.save_preference(&key, &corrupt).unwrap();
    assert_eq!(
        store.legacy_agent_backup(&id).unwrap_err().code,
        "legacy-agent-backup-corrupt"
    );
    assert_eq!(
        store.backup_legacy_agent(&value).unwrap_err().code,
        "legacy-agent-backup-conflict"
    );
    assert_eq!(store.preferences(&key).unwrap(), corrupt);
    assert!(store.legacy_agent_backup("../bad").is_err());
    let mut invalid: Snapshot = value;
    invalid.identity.len += 1;
    assert!(store.backup_legacy_agent(&invalid).is_err());
    assert_eq!(fs::read(file).unwrap(), b"\xef\xbb\xbf123\r\n");
}
#[test]
fn ambiguous_paths_hardlinks_and_oversized_state_are_rejected() {
    let (_temp, file) = fixture();
    // Preserve traversal in the input; Windows verbatim Path::join would
    // otherwise resolve it before snapshot gets a chance to reject it.
    let separator = std::path::MAIN_SEPARATOR;
    let parent = PathBuf::from(format!(
        "{}{separator}..{separator}data{separator}agent.pid",
        file.parent().unwrap().display()
    ));
    assert!(parent
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir)));
    assert!(agent::snapshot(&parent).is_err());
    assert!(agent::snapshot(&file.with_file_name("config.json")).is_err());
    let other = file.with_extension("linked");
    fs::hard_link(&file, &other).unwrap();
    assert!(agent::snapshot(&file).is_err());
    fs::remove_file(other).unwrap();
    fs::write(&file, vec![0; 8193]).unwrap();
    assert!(agent::snapshot(&file).is_err());
    #[cfg(unix)]
    {
        let link = file.with_file_name("agent-heartbeat.txt");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(agent::snapshot(&link).is_err());
    }
}
#[cfg(windows)]
#[test]
fn windows_rename_recovery_keeps_object_and_never_overwrites_new_state() {
    let (_temp, file) = fixture();
    let value = agent::snapshot(&file).unwrap();
    let operation = "c".repeat(64);
    let backup = value.archive_path(&operation).unwrap();
    agent::windows::archive(&value, &operation).unwrap();
    assert!(!file.exists());
    assert_eq!(fs::read(&backup).unwrap(), value.contents);
    // No plan receipt yet: recover from the exact archived object.
    agent::windows::archive(&value, &operation).unwrap();
    fs::write(&file, b"new user state").unwrap();
    assert!(agent::windows::restore(&value, &operation).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"new user state");
    assert_eq!(fs::read(&backup).unwrap(), value.contents);
    fs::remove_file(&file).unwrap();
    agent::windows::restore(&value, &operation).unwrap();
    assert_eq!(agent::snapshot(&file).unwrap(), value);
    agent::windows::restore(&value, &operation).unwrap();
    assert!(!backup.exists());
    fs::write(&backup, b"foreign backup").unwrap();
    assert!(agent::windows::archive(&value, &operation).is_err());
    assert_eq!(fs::read(&backup).unwrap(), b"foreign backup");
    assert_eq!(agent::snapshot(&file).unwrap(), value);
    fs::remove_file(&file).unwrap();
    fs::remove_file(&backup).unwrap();
    assert!(agent::windows::archive(&value, &operation).is_err());
    assert!(agent::windows::restore(&value, &operation).is_err());
}
