use std::{cell::Cell, fs};
use tcm_core::{lifecycle::*, startup_file::StartupFile, store::Store, *};
struct Native {
    enabled: Cell<bool>,
    failure: Cell<bool>,
}
impl Autostart for Native {
    fn enabled(&self) -> Result<bool> {
        Ok(self.enabled.get())
    }
    fn set(&self, value: bool) -> Result<()> {
        self.enabled.set(value);
        if self.failure.replace(false) {
            Err(AppError::new(
                "native-refused",
                "Injected failure after native mutation",
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn interrupted_native_setting_commits_on_restart_and_replayed_operation_does_not_reapply() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db.sqlite");
    let store = Store::open(&path).unwrap();
    let desired = Settings {
        launch_at_login: true,
        close_action: CloseAction::Background,
        ..Default::default()
    };
    store
        .prepare_lifecycle("login", desired.clone(), false)
        .unwrap();
    drop(store);
    let native = Native {
        enabled: Cell::new(true),
        failure: Cell::new(false),
    };
    let store = Store::open(&path).unwrap();
    reconcile(&store, &native).unwrap();
    assert_eq!(store.lifecycle_settings().unwrap().revision, 1);
    native.enabled.set(false);
    let replay = apply(&store, "login", desired, &native).unwrap();
    assert_eq!(replay.phase, "committed");
    assert!(!native.enabled.get());
}
#[test]
fn native_partial_failure_restores_previous_setting_and_does_not_commit_preferences() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("db.sqlite")).unwrap();
    let native = Native {
        enabled: Cell::new(false),
        failure: Cell::new(true),
    };
    let desired = Settings {
        launch_at_login: true,
        ..Default::default()
    };
    assert_eq!(
        apply(&store, "login", desired, &native).unwrap_err().code,
        "native-refused"
    );
    assert!(!native.enabled.get());
    assert_eq!(store.lifecycle_settings().unwrap().revision, 0);
}
#[test]
fn login_files_preserve_unicode_ampersand_and_reject_another_owner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let dir = root.join("LaunchAgents");
    let exe = root.join("中文 & app/Manager");
    let file = StartupFile::new("macos", &dir, "test.itm", &exe, "ITM").unwrap();
    file.set(true).unwrap();
    assert!(file.enabled().unwrap());
    let value = plist::Value::from_file(&file.path).unwrap();
    let args = value.as_dictionary().unwrap()["ProgramArguments"]
        .as_array()
        .unwrap();
    assert_eq!(args[0].as_string().unwrap(), exe.to_str().unwrap());
    file.set(false).unwrap();
    assert!(!file.enabled().unwrap());
    fs::write(&file.path, b"external registration").unwrap();
    assert!(file.set(true).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), b"external registration");
}
#[cfg(target_os = "linux")]
#[test]
fn gio_launches_appimage_style_path_with_reserved_characters_as_one_argument() {
    use std::{
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{Duration, Instant},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let bin = root.join("中文 $money %percent `ticks` \"quotes\"");
    fs::create_dir(&bin).unwrap();
    let exe = bin.join("echo app");
    fs::write(
        &exe,
        b"#!/bin/sh\nprintf '%s' \"$1\" > \"$STARTUP_ACCEPTANCE_REPORT\"\n",
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let entry =
        StartupFile::new("linux", &root.join("autostart"), "test.itm", &exe, "ITM").unwrap();
    entry.set(true).unwrap();
    let report = root.join("arguments");
    let output = Command::new("gio")
        .arg("launch")
        .arg(&entry.path)
        .env("STARTUP_ACCEPTANCE_REPORT", &report)
        .output()
        .expect("gio is required for Linux desktop acceptance");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !report.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(report).unwrap(), "--background");
    entry.set(false).unwrap();
}

#[cfg(unix)]
#[test]
fn dangling_login_symlink_is_never_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = StartupFile::new("macos", &root, "test.itm", &root.join("Manager"), "ITM").unwrap();
    let target = root.join("external-missing");
    std::os::unix::fs::symlink(&target, &file.path).unwrap();
    assert!(file.set(true).is_err());
    assert_eq!(fs::read_link(&file.path).unwrap(), target);
    assert!(!target.exists());
}
