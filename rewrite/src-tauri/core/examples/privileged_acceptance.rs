//! Disposable cloud check of the privileged process and protected files.
//! Executed by the runner's administrator. This does not claim a polkit dialog
//! or full Manager desktop acceptance, and installs no authorization policy.
#[cfg(unix)]
mod check {
    use std::{
        fs,
        io::{Read, Write},
        os::unix::{fs::PermissionsExt, process::CommandExt},
        path::Path,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    use tcm_core::{
        emby::{self, PublicIndex},
        maintenance::{self, Action, Client, Outcome},
        maintenance_pipe::Pipe,
    };
    struct Duplex {
        input: Pipe,
        output: Pipe,
    }
    impl Read for Duplex {
        fn read(&mut self, v: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(v)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, v: &[u8]) -> std::io::Result<usize> {
            self.output.write(v)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.output.flush()
        }
    }
    fn start(web: &Path) -> (Client<Duplex>, Child) {
        let helper = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("tcm-maintenance-helper");
        let mut child = Command::new(helper)
            .args([
                "--serve",
                &std::process::id().to_string(),
                web.to_str().unwrap(),
            ])
            .env("PKEXEC_UID", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let pipe = Duplex {
            input: Pipe::new(child.stdout.take().unwrap().into(), Duration::from_secs(30)).unwrap(),
            output: Pipe::new(child.stdin.take().unwrap().into(), Duration::from_secs(30)).unwrap(),
        };
        (Client::from_authenticated_transport(pipe), child)
    }
    fn ask(client: &mut Client<Duplex>, command: maintenance::Command) -> Outcome {
        let pipes = client.transport_mut().unwrap();
        pipes.input.reset(Duration::from_secs(30));
        pipes.output.reset(Duration::from_secs(30));
        match client.request(command).unwrap() {
            Outcome::Failed(e) => panic!("{e:?}"),
            outcome => outcome,
        }
    }
    fn wait(child: &mut Child, success: bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.success(), success, "{status}");
                return;
            }
            assert!(Instant::now() < deadline, "permission child did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn plan(
        client: &mut Client<Duplex>,
        id: &str,
        action: Action,
        index: &PublicIndex,
    ) -> emby::MaintenancePlan {
        match ask(
            client,
            maintenance::Command::Plan {
                id: id.into(),
                action,
                index: index.clone(),
            },
        ) {
            Outcome::Plan(p) => p,
            _ => panic!("wrong plan reply"),
        }
    }
    fn apply(client: &mut Client<Duplex>, p: emby::MaintenancePlan) {
        assert!(matches!(
            ask(
                client,
                maintenance::Command::Apply {
                    id: p.id,
                    fingerprint: p.fingerprint
                }
            ),
            Outcome::Applied(_)
        ));
    }
    pub fn run() {
        assert_eq!(std::env::consts::OS, "linux");
        assert_eq!(
            unsafe { libc::geteuid() },
            0,
            "Run only in disposable cloud validation with administrator authorization"
        );
        // Hosted runners may make /opt writable for tool installation. Use the
        // same protected system parent as the real recovery journal.
        let root =
            Path::new("/var/lib").join(format!("tcm-permission-check-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let web = root.join("web");
        fs::create_dir(&web).unwrap();
        fs::set_permissions(&web, fs::Permissions::from_mode(0o755)).unwrap();
        let original = format!(
            "<html><head></head><body>{}</body></html>",
            "original Emby ".repeat(40)
        );
        fs::write(web.join("index.html"), &original).unwrap();
        fs::set_permissions(web.join("index.html"), fs::Permissions::from_mode(0o644)).unwrap();
        let nfo = root.join("read-only.nfo");
        fs::write(&nfo, b"\xef\xbb\xbf<movie><tag>External</tag></movie>\r\n").unwrap();
        let nfo_bytes = fs::read(&nfo).unwrap();
        let nfo_time = fs::metadata(&nfo).unwrap().modified().unwrap();
        let uid = std::env::var("SUDO_UID").unwrap().parse::<u32>().unwrap();
        assert_ne!(uid, 0);
        assert!(!Command::new("/usr/bin/touch")
            .arg(web.join("forbidden"))
            .uid(uid)
            .status()
            .unwrap()
            .success());
        assert!(!web.join("forbidden").exists());
        let index = emby::public_index(&[], emby::timestamp());
        let (mut client, mut child) = start(&web);
        assert!(matches!(
            ask(&mut client, maintenance::Command::Status {}),
            Outcome::Status { .. }
        ));
        let p = plan(&mut client, "cloud-install", Action::Install, &index);
        apply(&mut client, p);
        assert!(matches!(
            ask(
                &mut client,
                maintenance::Command::Start {
                    session: "cloud-service".into()
                }
            ),
            Outcome::Service(_)
        ));
        std::thread::sleep(Duration::from_millis(2200));
        let runtime: serde_json::Value =
            serde_json::from_slice(&fs::read(web.join("technical-specs-runtime.json")).unwrap())
                .unwrap();
        assert_eq!(runtime["enabled"], true);
        drop(client);
        wait(&mut child, true); // actual inherited-pipe EOF, not a mock
        let runtime: serde_json::Value =
            serde_json::from_slice(&fs::read(web.join("technical-specs-runtime.json")).unwrap())
                .unwrap();
        assert_eq!(runtime["enabled"], false);
        let (mut client, mut child) = start(&web);
        match ask(
            &mut client,
            maintenance::Command::Operation {
                id: "cloud-install".into(),
            },
        ) {
            Outcome::Operation(p) => assert_eq!(p.phase, "committed"),
            _ => panic!("missing durable receipt"),
        };
        let p = plan(&mut client, "cloud-remove", Action::Remove, &index);
        apply(&mut client, p);
        assert_eq!(
            fs::read(web.join("index.html")).unwrap(),
            original.as_bytes()
        );
        let p = plan(&mut client, "cloud-lost-reply", Action::Install, &index);
        // Four requests precede this: Operation, Plan/remove, Apply/remove, Plan/install:
        // the manual fifth frame is committed even though its reply cannot arrive.
        let pipes = client.transport_mut().unwrap();
        maintenance::write_frame(
            &mut pipes.output,
            &maintenance::Request {
                version: 1,
                sequence: 5,
                request: maintenance::Command::Apply {
                    id: p.id,
                    fingerprint: p.fingerprint,
                },
            },
        )
        .unwrap();
        drop(client);
        wait(&mut child, false);
        let (mut client, mut child) = start(&web);
        match ask(
            &mut client,
            maintenance::Command::Operation {
                id: "cloud-lost-reply".into(),
            },
        ) {
            Outcome::Operation(p) => assert_eq!(p.phase, "committed"),
            _ => panic!("lost reply receipt not recovered"),
        };
        let p = plan(&mut client, "cloud-final-remove", Action::Remove, &index);
        apply(&mut client, p);
        ask(&mut client, maintenance::Command::Close {});
        drop(client);
        wait(&mut child, true);
        assert_eq!(
            fs::read(web.join("index.html")).unwrap(),
            original.as_bytes()
        );
        assert_eq!(fs::read(&nfo).unwrap(), nfo_bytes);
        assert_eq!(fs::metadata(&nfo).unwrap().modified().unwrap(), nfo_time);
        let outside = root.join("outside");
        fs::write(&outside, b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, web.join("technical-specs-card.js")).unwrap();
        let (mut client, mut child) = start(&web);
        assert!(
            matches!(client.request(maintenance::Command::Status {}),Ok(Outcome::Failed(error)) if error.code=="maintenance-untrusted-path")
        );
        drop(client);
        wait(&mut child, false);
        assert_eq!(fs::read(outside).unwrap(), b"keep");
        let journal = Path::new("/var/lib/tech-card-manager-validation")
            .join(tcm_core::hash(web.to_string_lossy().as_bytes()));
        fs::remove_dir_all(journal).unwrap();
        fs::remove_dir_all(&root).unwrap();
        println!(
            "{}",
            serde_json::json!({"status":"passed","target":std::env::consts::ARCH,"checks":["ordinary-user-write-denied","native-root-helper-started","protected-journal-install","lease-renewal","pipe-eof-stops-service-and-child","restart-recovers-receipt","lost-apply-reply-recovers-without-repeat","remove-restores-original-html","nfo-bytes-and-mtime-preserved","symlink-target-refused","isolated-files-cleaned"],"limitations":["administrator cloud fixture; not interactive polkit or Manager desktop acceptance","no production Emby installation used"]})
        );
    }
}
fn main() {
    #[cfg(unix)]
    check::run();
    #[cfg(not(unix))]
    panic!("Linux acceptance only");
}
