use super::*;
use product_core::emby;
use std::{
    sync::{mpsc, Arc},
    thread::JoinHandle,
    time::{Duration, Instant},
};
pub trait Owner: Send + 'static {
    fn request(&mut self, command: Command) -> Result<Outcome>;
    fn finish(&mut self) -> Result<()>;
}
struct Work {
    command: Command,
    reply: mpsc::Sender<Result<Outcome>>,
}
pub struct Connection {
    sender: Option<mpsc::Sender<Work>>,
    worker: Option<JoinHandle<Result<()>>>,
}
impl Connection {
    pub fn request(&self, command: Command) -> Result<Outcome> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| {
                AppError::new("maintenance-disconnected", "Authorization session closed")
            })?
            .send(Work { command, reply: tx })
            .map_err(|e| AppError::new("maintenance-disconnected", e))?;
        rx.recv_timeout(Duration::from_secs(35)).map_err(|e| {
            AppError::new(
                "maintenance-result-unverified",
                format!("{e}; query the original operation after reconnecting"),
            )
        })?
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.sender = None;
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| AppError::new("maintenance-worker", "Permission worker panicked"))?,
            None => Ok(()),
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("maintenance-cleanup: {error}");
        }
    }
}
pub fn launch(mut owner: impl Owner, store: Arc<product_core::store::Store>) -> Result<Connection> {
    let (tx, rx) = mpsc::channel::<Work>();
    let worker = std::thread::Builder::new()
        .name("emby-permission".into())
        .spawn(move || {
            let mut revision = None;
            let mut next_tick = Instant::now() + Duration::from_secs(2);
            let mut publish_error: Option<AppError> = None;
            loop {
                match rx.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
                    Ok(work) => {
                        let start = matches!(work.command, Command::Start { .. });
                        if start {
                            revision = None;
                            publish_error = None;
                        }
                        let mut result = owner.request(work.command);
                        if let (Some(error), Ok(Outcome::Status { service, .. })) =
                            (&publish_error, &mut result)
                        {
                            service.phase = "failed".into();
                            service.error = Some(error.clone());
                        }
                        let _ = work.reply.send(result);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= next_tick {
                    next_tick = Instant::now() + Duration::from_secs(2);
                    let status = owner.request(Command::Status {});
                    match status {
                        Ok(Outcome::Status {
                            service,
                            integration,
                        }) if service.active() => {
                            let publication = (|| -> Result<()> {
                                let actual = integration
                                    .details
                                    .as_ref()
                                    .and_then(|details| details.data_fingerprint.as_ref());
                                if integration
                                    .details
                                    .as_ref()
                                    .is_some_and(|details| !details.data_exists)
                                {
                                    revision = None;
                                }
                                let current = store.index_current(actual.map(String::as_str))?;
                                // Disable a previous root set before publishing or waiting
                                // for a new complete scan. The helper rechecks file identity.
                                owner.request(Command::IndexCurrent {
                                    fingerprint: if current { actual.cloned() } else { None },
                                })?;
                                if let Some((next, items)) = store.publication_snapshot(revision)? {
                                    owner.request(Command::Publish {
                                        index: emby::public_index(&items, emby::timestamp()),
                                    })?;
                                    revision = Some(next);
                                }
                                Ok(())
                            })();
                            if let Err(error) = publication {
                                publish_error = Some(error);
                                let _ = owner.request(Command::Stop {});
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            owner.finish()
        })
        .map_err(|e| AppError::new("maintenance-worker", e))?;
    Ok(Connection {
        sender: Some(tx),
        worker: Some(worker),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    struct Recorded {
        commands: Mutex<Vec<String>>,
        finished: AtomicUsize,
    }
    struct Fixture {
        recorded: Arc<Recorded>,
        running: bool,
        fail_publish: bool,
        active_phase: &'static str,
    }
    impl Owner for Fixture {
        fn request(&mut self, command: Command) -> Result<Outcome> {
            let name = match command {
                Command::Status {} => "status",
                Command::Start { .. } => "start",
                Command::Stop {} => "stop",
                Command::Publish { .. } => "publish",
                Command::IndexCurrent { .. } => "index-current",
                _ => "unexpected",
            };
            self.recorded.commands.lock().unwrap().push(name.into());
            match command {
                Command::Start { .. } => {
                    self.running = true;
                    Ok(Outcome::Closed)
                }
                Command::Stop {} => {
                    self.running = false;
                    Ok(Outcome::Service(
                        product_core::card_service::ServiceStatus {
                            phase: "stopped".into(),
                            last_started_at: None,
                            lease: None,
                            error: None,
                        },
                    ))
                }
                Command::Publish { .. } if self.fail_publish => {
                    Err(AppError::new("fixture-publication", "Publication failed"))
                }
                Command::Publish { .. } => Ok(Outcome::Published(true)),
                Command::IndexCurrent { .. } => Ok(Outcome::Closed),
                Command::Status {} => Ok(Outcome::Status {
                    integration: emby::IntegrationStatus {
                        target: "fixture".into(),
                        installed: true,
                        healthy: true,
                        phase: "installed".into(),
                        issues: vec![],
                        requires_permission: false,
                        details: None,
                        legacy_patch: None,
                    },
                    service: product_core::card_service::ServiceStatus {
                        phase: if self.running {
                            self.active_phase
                        } else {
                            "stopped"
                        }
                        .into(),
                        last_started_at: None,
                        lease: None,
                        error: None,
                    },
                }),
                _ => Err(AppError::new("fixture-command", "Unexpected command")),
            }
        }
        fn finish(&mut self) -> Result<()> {
            self.running = false;
            self.recorded.finished.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    fn fixture(fail_publish: bool) -> (tempfile::TempDir, Connection, Arc<Recorded>) {
        fixture_with_phase(fail_publish, "running")
    }
    fn fixture_with_phase(
        fail_publish: bool,
        active_phase: &'static str,
    ) -> (tempfile::TempDir, Connection, Arc<Recorded>) {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(
            product_core::store::Store::open(
                &temp.path().canonicalize().unwrap().join("state.sqlite"),
            )
            .unwrap(),
        );
        let recorded = Arc::new(Recorded {
            commands: Mutex::new(vec![]),
            finished: AtomicUsize::new(0),
        });
        let connection = launch(
            Fixture {
                recorded: recorded.clone(),
                running: false,
                fail_publish,
                active_phase,
            },
            store,
        )
        .unwrap();
        (temp, connection, recorded)
    }
    fn wait_for(recorded: &Recorded, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !recorded.commands.lock().unwrap().iter().any(|s| s == name) {
            assert!(Instant::now() < deadline, "worker did not reach {name}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn helper_worker_recreates_missing_owned_index_without_reinstalling_the_card() {
        struct LocalOwner {
            session: product_core::maintenance::Session,
            sequence: u64,
        }
        impl Owner for LocalOwner {
            fn request(&mut self, request: Command) -> Result<Outcome> {
                self.sequence += 1;
                self.session
                    .dispatch(product_core::maintenance::Request {
                        version: 1,
                        sequence: self.sequence,
                        request,
                    })
                    .map(|reply| reply.outcome)
            }
            fn finish(&mut self) -> Result<()> {
                self.request(Command::Close {}).map(|_| ())
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let media = directory.join("movies");
        let web = directory.join("web");
        std::fs::create_dir(&media).unwrap();
        std::fs::create_dir(&web).unwrap();
        let html = format!(
            "<html><head></head><body>{}</body></html>",
            "Emby ".repeat(80)
        );
        std::fs::write(web.join("index.html"), &html).unwrap();
        std::fs::write(media.join("movie.nfo"), "<movie><title>Fixture</title><uniqueid type=\"imdb\">tt1234567</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>Fixture</item></section></technicalspecs></movie>").unwrap();
        let store =
            Arc::new(product_core::store::Store::open(&directory.join("state.sqlite")).unwrap());
        store
            .configure(
                "roots",
                product_core::Configuration {
                    roots: vec![product_core::LibraryRoot {
                        id: "movies".into(),
                        path: media.to_string_lossy().into(),
                        space: product_core::Space::Movie,
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .submit(product_core::ScanRequest {
                operation_id: "first".into(),
                space: product_core::Space::Movie,
                root_ids: vec!["movies".into()],
            })
            .unwrap();
        store.run_next(|| false, |_| {}).unwrap();
        let owner = LocalOwner {
            session: product_core::maintenance::Session::open(&web, &directory.join("backups"))
                .unwrap(),
            sequence: 0,
        };
        let mut connection = launch(owner, store).unwrap();
        connection
            .request(Command::Start {
                session: "index-only".into(),
            })
            .unwrap();
        let data_path = web.join("technical-specs-data.json");
        let wait = || {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                let Outcome::Status { service, .. } =
                    connection.request(Command::Status {}).unwrap()
                else {
                    panic!("status");
                };
                assert_ne!(service.phase, "failed", "{:?}", service.error);
                if service.phase == "running" && data_path.is_file() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "index did not recover: {service:?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait();
        let before: emby::PublicIndex =
            serde_json::from_slice(&std::fs::read(&data_path).unwrap()).unwrap();
        std::fs::remove_file(&data_path).unwrap();
        wait();
        let after: emby::PublicIndex =
            serde_json::from_slice(&std::fs::read(&data_path).unwrap()).unwrap();
        assert_eq!(
            emby::index_fingerprint(&before).unwrap(),
            emby::index_fingerprint(&after).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(web.join("index.html")).unwrap(),
            html
        );
        assert!(!web.join("technical-specs-card.js").exists());
        connection.shutdown().unwrap();
        let lease: emby::Lease = serde_json::from_slice(
            &std::fs::read(web.join("technical-specs-runtime.json")).unwrap(),
        )
        .unwrap();
        assert!(!lease.enabled);
    }
    #[test]
    fn initial_index_failure_stops_helper_and_retry_uses_the_same_connection() {
        let (_temp, mut connection, recorded) = fixture_with_phase(false, "starting");
        for id in ["first", "retry"] {
            connection
                .request(Command::Start { session: id.into() })
                .unwrap();
            let Outcome::Status { service, .. } = connection.request(Command::Status {}).unwrap()
            else {
                panic!("status expected")
            };
            assert_eq!(service.phase, "starting");
            let failed = crate::emby::initial_index_failure(
                service,
                AppError::new("scan-fixture", "initial failure"),
                || crate::emby::stop_remote_service(|command| connection.request(command)),
            );
            assert_eq!(failed.phase, "error");
            assert_eq!(failed.error.unwrap().code, "scan-fixture");
            let Outcome::Status { service, .. } = connection.request(Command::Status {}).unwrap()
            else {
                panic!("status expected")
            };
            assert_eq!(service.phase, "stopped");
        }
        connection.shutdown().unwrap();
        connection.shutdown().unwrap();
        let commands = recorded.commands.lock().unwrap();
        assert_eq!(commands.iter().filter(|name| *name == "stop").count(), 2);
        assert!(!commands.iter().any(|name| name == "publish"));
        assert_eq!(recorded.finished.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn one_publisher_stops_on_failure_and_reports_the_original_reason() {
        let (_temp, mut connection, recorded) = fixture(true);
        connection
            .request(Command::Start {
                session: "fixture".into(),
            })
            .unwrap();
        wait_for(&recorded, "stop");
        match connection.request(Command::Status {}).unwrap() {
            Outcome::Status { service, .. } => {
                assert_eq!(service.phase, "failed");
                assert_eq!(service.error.unwrap().code, "fixture-publication");
            }
            _ => panic!("status expected"),
        }
        connection.shutdown().unwrap();
        connection.shutdown().unwrap();
        let commands = recorded.commands.lock().unwrap();
        assert_eq!(commands.iter().filter(|c| *c == "publish").count(), 1);
        assert_eq!(commands.iter().filter(|c| *c == "stop").count(), 1);
        assert_eq!(recorded.finished.load(Ordering::SeqCst), 1);
        assert_eq!(
            connection.request(Command::Status {}).unwrap_err().code,
            "maintenance-disconnected"
        );
    }
    #[test]
    fn initial_waiting_service_still_checks_permission_and_publishes() {
        let (_temp, mut connection, recorded) = fixture_with_phase(false, "starting");
        connection
            .request(Command::Start {
                session: "waiting".into(),
            })
            .unwrap();
        wait_for(&recorded, "publish");
        connection.shutdown().unwrap();
        let commands = recorded.commands.lock().unwrap();
        let check = commands
            .iter()
            .position(|name| name == "index-current")
            .unwrap();
        let publish = commands.iter().position(|name| name == "publish").unwrap();
        assert!(check < publish);
        assert_eq!(recorded.finished.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn explicit_shutdown_joins_the_worker_and_prevents_later_publication() {
        let (_temp, mut connection, recorded) = fixture(false);
        connection
            .request(Command::Start {
                session: "fixture".into(),
            })
            .unwrap();
        wait_for(&recorded, "publish");
        connection.shutdown().unwrap();
        let count = recorded.commands.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(recorded.commands.lock().unwrap().len(), count);
        assert_eq!(recorded.finished.load(Ordering::SeqCst), 1);
    }
}
