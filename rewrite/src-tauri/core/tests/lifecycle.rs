use std::{cell::Cell, fs};
use tcm_core::{lifecycle::*, startup_file::StartupFile, store::Store, *};
struct Native {
    enabled: Cell<bool>,
    failure: Cell<bool>,
}
#[test]
fn deprecated_agent_entry_uses_the_original_first_switch_without_lossy_argument_decoding() {
    for arguments in [
        vec!["Manager.exe", "--agent"],
        vec!["Manager.exe", "--agent", "ignored"],
    ] {
        assert!(ignored_agent_launch(arguments));
    }
    for arguments in [
        vec![],
        vec!["--agent"],
        vec!["Manager.exe"],
        vec!["Manager.exe", "--Agent"],
        vec!["Manager.exe", "--agent=true"],
        vec!["Manager.exe", "--login-startup", "--agent"],
    ] {
        assert!(!ignored_agent_launch(arguments));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let malformed = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
        assert!(ignored_agent_launch([malformed.clone(), "--agent".into()]));
        assert!(!ignored_agent_launch(["Manager.exe".into(), malformed]));
    }
}
#[test]
fn silent_start_defaults_off_and_applies_only_to_explicit_login_launches() {
    let mut settings = Settings::default();
    assert!(!settings.start_hidden);
    for args in [vec![], vec!["--background"], vec!["--login-startup"]] {
        assert!(!silent_login_launch(&settings, args));
    }
    settings.start_hidden = true;
    assert!(silent_login_launch(&settings, ["--background"]));
    assert!(silent_login_launch(&settings, ["--login-startup"]));
    for args in [
        vec![],
        vec!["--login-startup=false"],
        vec!["--background-extra"],
    ] {
        assert!(!silent_login_launch(&settings, args));
    }
}

#[test]
fn original_application_choices_restore_without_registering_startup_and_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let old = root.join("old");
    fs::create_dir(&old).unwrap();
    let bytes=br#"{"auto_start":true,"auto_start_configured":true,"silent_start":true,"language":"en-US"}"#;
    fs::write(old.join("settings.json"), bytes).unwrap();
    let modified = fs::metadata(old.join("settings.json"))
        .unwrap()
        .modified()
        .unwrap();
    let store = Store::open(&root.join("new.sqlite")).unwrap();
    let plan = store
        .prepare_migration("application", &old, "tcm-portable")
        .unwrap();
    store
        .apply_migration("application", &plan.fingerprint)
        .unwrap();
    let settings = store.lifecycle_settings().unwrap();
    assert!(settings.start_hidden);
    assert!(settings.launch_at_login);
    assert_eq!(settings.revision, 1);
    let native = Native {
        enabled: Cell::new(false),
        failure: Cell::new(true),
    };
    reconcile(&store, &native).unwrap();
    assert!(!native.enabled.get());
    assert!(native.failure.get());
    assert!(store.tasks().unwrap().is_empty());
    drop(store);
    let store = Store::open(&root.join("new.sqlite")).unwrap();
    assert_eq!(store.lifecycle_settings().unwrap(), settings);
    assert_eq!(fs::read(old.join("settings.json")).unwrap(), bytes);
    assert_eq!(
        fs::metadata(old.join("settings.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    assert!(silent_login_launch(&settings, ["--login-startup"]));
}

#[test]
fn current_or_pending_application_choices_are_not_overwritten_by_an_import() {
    for pending in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let old = root.join("old");
        fs::create_dir(&old).unwrap();
        fs::write(
            old.join("settings.json"),
            br#"{"auto_start":true,"silent_start":true}"#,
        )
        .unwrap();
        let store = Store::open(&root.join("new.sqlite")).unwrap();
        let native = Native {
            enabled: Cell::new(false),
            failure: Cell::new(false),
        };
        let desired = Settings::default();
        if pending {
            store
                .prepare_lifecycle("user-choice", desired, false)
                .unwrap();
        } else {
            apply(&store, "user-choice", desired, &native).unwrap();
        }
        let before = store.lifecycle_settings().unwrap();
        let plan = store
            .prepare_migration("import", &old, "tcm-portable")
            .unwrap();
        store.apply_migration("import", &plan.fingerprint).unwrap();
        assert_eq!(store.lifecycle_settings().unwrap(), before);
        assert!(!native.enabled.get());
    }
}

#[test]
fn ambiguous_or_invalid_application_settings_roll_back_the_entire_import() {
    for conflicting in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let old = root.join("old");
        fs::create_dir(&old).unwrap();
        fs::write(
            old.join("settings.json"),
            if conflicting {
                br#"{"silent_start":true,"interval_seconds":300}"#.as_slice()
            } else {
                br#"{"silent_start":"true","interval_seconds":300}"#.as_slice()
            },
        )
        .unwrap();
        if conflicting {
            fs::write(old.join("config.json"), br#"{"silent_start":false}"#).unwrap();
        }
        let store = Store::open(&root.join("new.sqlite")).unwrap();
        let plan = store
            .prepare_migration("bad", &old, "tcm-portable")
            .unwrap();
        assert_eq!(
            store
                .apply_migration("bad", &plan.fingerprint)
                .unwrap_err()
                .code,
            "migration-application-settings"
        );
        assert_eq!(store.configuration().unwrap().revision, 0);
        assert_eq!(store.lifecycle_settings().unwrap(), Settings::default());
        assert_eq!(store.incremental_settings().unwrap().interval_seconds, 60);
        assert!(store.legacy_artifact("bad", "settings.json").is_err());
    }
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
