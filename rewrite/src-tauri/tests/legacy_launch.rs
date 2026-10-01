use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn built_manager_ignores_the_deprecated_agent_entry_before_application_setup() {
    for arguments in [
        vec!["--agent"],
        vec!["--agent", "ignored-trailing-argument"],
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_tcm-validation"))
            .args(arguments)
            .current_dir(temporary.path())
            .env(
                "REWRITE_PROBE_REPORT",
                temporary.path().join("events.jsonl"),
            )
            .env_remove("REWRITE_PROBE_AUTOCLOSE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("deprecated Agent launch did not exit before application setup");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output.status);
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap().trim(),
            "ignored deprecated --agent launch; open the visual Manager instead"
        );
        assert_eq!(
            std::fs::read_dir(temporary.path()).unwrap().count(),
            0,
            "application setup/probe must not run"
        );
    }
}
