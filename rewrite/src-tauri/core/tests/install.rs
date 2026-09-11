use std::fs;
use tcm_core::install::*;
#[test]
fn staged_candidate_keeps_original_until_commit_and_retains_backup() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let app = base.join("TCM.AppImage");
    let journal = base.join("update.json");
    fs::write(&app, b"old application").unwrap();
    let staged = stage(&journal, "one", &app, b"new application", "appimage").unwrap();
    assert_eq!(fs::read(&app).unwrap(), b"old application");
    let installed = commit(&journal).unwrap();
    assert_eq!(installed.phase, "installed-awaiting-health");
    assert_eq!(fs::read(&app).unwrap(), b"new application");
    assert_eq!(fs::read(staged.backup).unwrap(), b"old application");
}
#[test]
fn interruption_between_moves_restores_the_old_application() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let app = base.join("TCM.AppImage");
    let journal = base.join("update.json");
    fs::write(&app, b"old application").unwrap();
    let mut staged = stage(&journal, "one", &app, b"new application", "appimage").unwrap();
    staged.phase = "replacing".into();
    fs::write(&journal, serde_json::to_vec(&staged).unwrap()).unwrap();
    fs::rename(&app, &staged.backup).unwrap();
    assert_eq!(recover(&journal).unwrap().phase, "rolled-back");
    assert_eq!(fs::read(app).unwrap(), b"old application");
}
#[test]
fn external_application_change_is_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let app = base.join("TCM.AppImage");
    let journal = base.join("update.json");
    fs::write(&app, b"old application").unwrap();
    stage(&journal, "one", &app, b"new application", "appimage").unwrap();
    fs::write(&app, b"external repair").unwrap();
    assert!(commit(&journal).is_err());
    assert_eq!(fs::read(app).unwrap(), b"external repair");
}
#[test]
fn invalid_mac_archive_never_removes_the_old_application() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let app = base.join("TCM.app");
    let journal = base.join("update.json");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("original"), b"old application").unwrap();
    assert!(stage(&journal, "one", &app, b"not an application tarball", "app").is_err());
    assert_eq!(fs::read(app.join("original")).unwrap(), b"old application");
}
#[test]
fn interrupted_after_backup_keeps_live_target_and_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let target = base.join("AppImage");
    let journal = base.join("journal.json");
    fs::write(&target, b"old").unwrap();
    let mut plan = stage(&journal, "backup-crash", &target, b"new", "appimage").unwrap();
    plan.phase = "replacing".into();
    fs::write(&journal, serde_json::to_vec(&plan).unwrap()).unwrap();
    fs::hard_link(&target, &plan.backup).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(recover(&journal).unwrap().phase, "rolled-back");
    assert_eq!(fs::read(&target).unwrap(), b"old");
}
#[cfg(target_os = "macos")]
#[test]
fn mac_bundle_atomic_exchange_retains_complete_old_bundle() {
    use flate2::{write::GzEncoder, Compression};
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let target = base.join("App.app");
    let journal = base.join("journal.json");
    fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
    fs::write(target.join("Contents/Info.plist"), b"old plist").unwrap();
    fs::write(target.join("Contents/MacOS/app"), b"old executable").unwrap();
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, body) in [
        ("App.app/Contents/Info.plist", b"new plist".as_slice()),
        ("App.app/Contents/MacOS/app", b"new executable".as_slice()),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(&mut h, path, body).unwrap();
    }
    let bytes = tar.into_inner().unwrap().finish().unwrap();
    let plan = stage(&journal, "bundle", &target, &bytes, "app").unwrap();
    let reader_target = target.clone();
    let reading = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = reading.clone();
    let reader = std::thread::spawn(move || {
        while flag.load(std::sync::atomic::Ordering::SeqCst) {
            let b = fs::read(reader_target.join("Contents/MacOS/app"))
                .expect("target must never disappear");
            assert!(b == b"old executable" || b == b"new executable");
        }
    });
    commit(&journal).unwrap();
    reading.store(false, std::sync::atomic::Ordering::SeqCst);
    reader.join().unwrap();
    assert_eq!(
        fs::read(target.join("Contents/MacOS/app")).unwrap(),
        b"new executable"
    );
    assert_eq!(
        fs::read(plan.backup.join("Contents/MacOS/app")).unwrap(),
        b"old executable"
    );
}
#[test]
fn rollback_is_idempotent_and_refuses_external_edits() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let target = base.join("AppImage");
    let journal = base.join("update.json");
    fs::write(&target, b"old").unwrap();
    stage(&journal, "rollback", &target, b"new", "appimage").unwrap();
    commit(&journal).unwrap();
    fs::write(&target, b"external").unwrap();
    assert!(rollback(&journal).is_err());
    fs::write(&target, b"new").unwrap();
    assert_eq!(rollback(&journal).unwrap().phase, "rolled-back");
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(rollback(&journal).unwrap().phase, "rolled-back");
}
#[test]
fn appimage_validation_rejects_wrong_architecture_before_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let target = base.join("AppImage");
    let journal = base.join("update.json");
    fs::write(&target, b"old").unwrap();
    let mut bytes = vec![0; 64];
    bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
    bytes[8..11].copy_from_slice(b"AI\x02");
    bytes[18] = 62;
    let plan = stage(&journal, "arch", &target, &bytes, "appimage").unwrap();
    assert!(validate_candidate(&plan, "itm", "5.0.0", "x86_64").is_ok());
    assert_eq!(
        validate_candidate(&plan, "itm", "5.0.0", "aarch64")
            .unwrap_err()
            .code,
        "update-architecture"
    );
    assert_eq!(fs::read(&target).unwrap(), b"old");
}
