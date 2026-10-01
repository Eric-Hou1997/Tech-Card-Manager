use std::{fs, io::Cursor, path::PathBuf};
use tcm_core::{
    emby::{self, PublicIndex},
    maintenance::*,
    *,
};
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Session) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let web = root.join("web");
    let backup = root.join("helper-private");
    fs::create_dir(&web).unwrap();
    let html = format!(
        "\u{feff}<html><head><title>Emby</title></head>\r\n<body>{}</body></html>",
        "原始内容 ".repeat(50)
    );
    fs::write(web.join("index.html"), html).unwrap();
    fs::write(
        web.join("untouched.nfo"),
        "<movie><title>只读</title></movie>",
    )
    .unwrap();
    let session = Session::open(&web, &backup).unwrap();
    (temp, web, backup, session)
}
fn index() -> PublicIndex {
    emby::public_index(&[], "2026-09-12T00:00:00Z".into())
}
fn request(sequence: u64, request: Command) -> Request {
    Request {
        version: 1,
        sequence,
        request,
    }
}
fn install(session: &mut Session) -> String {
    let plan = session
        .dispatch(request(
            1,
            Command::Plan {
                id: "install".into(),
                action: Action::Install,
                index: index(),
            },
        ))
        .unwrap();
    let Outcome::Plan(plan) = plan.outcome else {
        panic!("plan failed")
    };
    let reply = session
        .dispatch(request(
            2,
            Command::Apply {
                id: plan.id,
                fingerprint: plan.fingerprint.clone(),
            },
        ))
        .unwrap();
    assert!(matches!(reply.outcome, Outcome::Applied(_)));
    plan.fingerprint
}
#[test]
fn helper_requires_a_matching_completed_index_before_enabling_display() {
    let (_temp, web, _backup, mut session) = fixture();
    install(&mut session);
    let Outcome::Service(started) = session
        .dispatch(request(
            3,
            Command::Start {
                session: "waiting".into(),
            },
        ))
        .unwrap()
        .outcome
    else {
        panic!("service")
    };
    assert!(!started.lease.unwrap().enabled);
    let fingerprint = emby::index_fingerprint(&index()).unwrap();
    for (sequence, expected, enabled) in [
        (4, Some("wrong".to_owned()), false),
        (5, Some(fingerprint), true),
        (6, None, false),
    ] {
        let Outcome::Service(state) = session
            .dispatch(request(
                sequence,
                Command::IndexCurrent {
                    fingerprint: expected,
                },
            ))
            .unwrap()
            .outcome
        else {
            panic!("lease")
        };
        assert_eq!(state.lease.unwrap().enabled, enabled);
        let actual: emby::Lease =
            serde_json::from_slice(&fs::read(web.join("technical-specs-runtime.json")).unwrap())
                .unwrap();
        assert_eq!(actual.enabled, enabled);
    }
    let read_lease = || {
        serde_json::from_slice::<emby::Lease>(
            &fs::read(web.join("technical-specs-runtime.json")).unwrap(),
        )
        .unwrap()
    };
    let previous = read_lease().sequence;
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert!(!read_lease().enabled);
    assert!(
        read_lease().sequence > previous,
        "withholding display must keep the session owner alive"
    );
    session
        .dispatch(request(
            7,
            Command::IndexCurrent {
                fingerprint: Some(emby::index_fingerprint(&index()).unwrap()),
            },
        ))
        .unwrap();
    let previous = read_lease().sequence;
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert!(read_lease().enabled);
    assert!(
        read_lease().sequence > previous,
        "resumed display must keep renewing"
    );
    let Outcome::Service(stopped) = session
        .dispatch(request(8, Command::Stop {}))
        .unwrap()
        .outcome
    else {
        panic!("stop")
    };
    assert_eq!(stopped.phase, "stopped");
    assert!(!stopped.lease.unwrap().enabled);
}
#[test]
fn helper_indexes_before_web_setup_and_repairs_without_restarting_the_session() {
    let (_temp, web, _backup, mut session) = fixture();
    let original = fs::read(web.join("index.html")).unwrap();
    let original_nfo = fs::read(web.join("untouched.nfo")).unwrap();
    let Outcome::Service(started) = session
        .dispatch(request(
            1,
            Command::Start {
                session: "index-only".into(),
            },
        ))
        .unwrap()
        .outcome
    else {
        panic!("start");
    };
    assert_eq!(started.phase, "starting");
    let published = index();
    assert!(matches!(
        session
            .dispatch(request(
                2,
                Command::Publish {
                    index: published.clone()
                }
            ))
            .unwrap()
            .outcome,
        Outcome::Published(true)
    ));
    let fingerprint = emby::index_fingerprint(&published).unwrap();
    let Outcome::Service(running) = session
        .dispatch(request(
            3,
            Command::IndexCurrent {
                fingerprint: Some(fingerprint),
            },
        ))
        .unwrap()
        .outcome
    else {
        panic!("running");
    };
    assert_eq!(running.phase, "running");
    assert_eq!(fs::read(web.join("index.html")).unwrap(), original);
    assert!(!web.join("technical-specs-card.js").exists());
    let data = fs::read(web.join("technical-specs-data.json")).unwrap();
    assert!(
        matches!(session.dispatch(request(4, Command::Repair { id: "repair".into() })).unwrap().outcome, Outcome::Applied(status) if status.healthy)
    );
    let Outcome::Status { service: after, .. } = session
        .dispatch(request(5, Command::Status {}))
        .unwrap()
        .outcome
    else {
        panic!("status");
    };
    assert_eq!(after.phase, "running");
    assert_eq!(after.last_started_at, running.last_started_at);
    assert_eq!(after.lease.unwrap().session_id, "index-only");
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
    let Outcome::Service(stopped) = session
        .dispatch(request(6, Command::Stop {}))
        .unwrap()
        .outcome
    else {
        panic!("stop");
    };
    assert_eq!(stopped.phase, "stopped");
    assert!(!stopped.lease.unwrap().enabled);
    fs::remove_file(web.join("technical-specs-data.json")).unwrap();
    session
        .dispatch(request(7, Command::Publish { index: published }))
        .unwrap();
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
    session.dispatch(request(8, Command::Close {})).unwrap();
    assert_eq!(fs::read(web.join("untouched.nfo")).unwrap(), original_nfo);
}
#[test]
fn fixed_target_plan_apply_replay_and_restart_preserve_original_files() {
    let (_temp, web, backup, mut session) = fixture();
    let before = fs::read(web.join("index.html")).unwrap();
    let nfo = fs::read(web.join("untouched.nfo")).unwrap();
    let mtime = fs::metadata(web.join("untouched.nfo"))
        .unwrap()
        .modified()
        .unwrap();
    let plan = session
        .dispatch(request(
            1,
            Command::Plan {
                id: "install".into(),
                action: Action::Install,
                index: index(),
            },
        ))
        .unwrap();
    let Outcome::Plan(plan) = plan.outcome else {
        panic!("plan failed")
    };
    assert_eq!(fs::read(web.join("index.html")).unwrap(), before);
    assert!(!web.join("technical-specs-card.js").exists());
    let wrong = session
        .dispatch(request(
            2,
            Command::Apply {
                id: plan.id.clone(),
                fingerprint: "unreviewed".into(),
            },
        ))
        .unwrap();
    assert!(matches!(wrong.outcome, Outcome::Failed(_)));
    assert!(!web.join("technical-specs-card.js").exists());
    let apply = request(
        3,
        Command::Apply {
            id: plan.id.clone(),
            fingerprint: plan.fingerprint.clone(),
        },
    );
    let first = session.dispatch(apply.clone()).unwrap();
    let replay = session.dispatch(apply).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert!(session.dispatch(request(3, Command::Close {})).is_err());
    assert_eq!(
        emby::clean_index(&fs::read(web.join("index.html")).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        fs::read(web.join("technical-specs-card.js")).unwrap(),
        include_bytes!("../../../web-card/technical-specs-card.js")
    );
    assert_eq!(fs::read(web.join("untouched.nfo")).unwrap(), nfo);
    assert_eq!(
        fs::metadata(web.join("untouched.nfo"))
            .unwrap()
            .modified()
            .unwrap(),
        mtime
    );
    drop(session);
    let mut session = Session::open(&web, &backup).unwrap();
    assert!(matches!(
        session
            .dispatch(request(
                1,
                Command::Apply {
                    id: plan.id,
                    fingerprint: plan.fingerprint
                }
            ))
            .unwrap()
            .outcome,
        Outcome::Applied(_)
    ));
}

#[test]
fn helper_adoption_binds_the_review_and_rejects_replaced_confirmation() {
    let (_temp, web, _backup, mut session) = fixture();
    let original = fs::read_to_string(web.join("index.html")).unwrap();
    fs::write(web.join("index.html"),original.replace("</body>","<!-- IMDbTechManager WebPatch BEGIN --><script src=\"technical-specs-card.js?v=4.1.0\"></script><!-- IMDbTechManager WebPatch END --></body>")).unwrap();
    fs::write(
        web.join("technical-specs-card.js"),
        include_bytes!("../../../web-card/technical-specs-card.js"),
    )
    .unwrap();
    let Outcome::Status { integration, .. } = session
        .dispatch(request(1, Command::Status {}))
        .unwrap()
        .outcome
    else {
        panic!("status")
    };
    let reviewed = integration.legacy_patch.unwrap().fingerprint;
    let command = Command::PlanAdoption {
        id: "reviewed".into(),
        reviewed: reviewed.clone(),
        index: index(),
    };
    let Outcome::Plan(plan) = session
        .dispatch(request(2, command.clone()))
        .unwrap()
        .outcome
    else {
        panic!("plan")
    };
    let Outcome::Plan(replayed) = session.dispatch(request(2, command)).unwrap().outcome else {
        panic!("replay")
    };
    assert_eq!(plan.fingerprint, replayed.fingerprint);
    let changed = session
        .dispatch(request(
            3,
            Command::PlanAdoption {
                id: plan.id.clone(),
                reviewed: "different".into(),
                index: index(),
            },
        ))
        .unwrap();
    assert!(matches!(changed.outcome,Outcome::Failed(error) if error.code=="operation-conflict"));
    let applied = session
        .dispatch(request(
            4,
            Command::Apply {
                id: plan.id,
                fingerprint: plan.fingerprint,
            },
        ))
        .unwrap();
    assert!(matches!(applied.outcome,Outcome::Applied(status) if status.healthy));
}
#[test]
fn protocol_rejects_arbitrary_paths_scripts_unknown_actions_and_unbounded_frames() {
    for raw in [
        r#"{"command":"status","web":"/other"}"#,
        r#"{"command":"execute","shell":"whoami"}"#,
        r#"{"command":"apply","id":"x","fingerprint":"x","script":"arbitrary"}"#,
        r#"{"command":"plan","id":"x","action":"delete-directory","index":{}}"#,
    ] {
        assert!(
            serde_json::from_str::<Command>(raw).is_err(),
            "accepted {raw}"
        );
    }
    assert!(read_frame(&mut Cursor::new(u32::MAX.to_be_bytes())).is_err());
    assert!(read_frame(&mut Cursor::new([0, 0, 0, 0])).is_err());
    assert!(read_frame(&mut Cursor::new([0, 0, 0, 5, 1])).is_err());
    assert!(read_frame(&mut Cursor::new([0, 0])).is_err());
    assert!(read_frame(&mut Cursor::new([])).unwrap().is_none());
    let (_temp, _web, _backup, mut session) = fixture();
    assert!(session.dispatch(request(2, Command::Status {})).is_err());
    let mut unsupported = request(1, Command::Status {});
    unsupported.version = 2;
    assert!(session.dispatch(unsupported).is_err());
    let mut invalid = index();
    invalid
        .items
        .insert("/media/private.nfo".into(), Specs::new());
    assert!(matches!(
        session
            .dispatch(request(1, Command::Publish { index: invalid }))
            .unwrap()
            .outcome,
        Outcome::Failed(_)
    ));
}
#[test]
fn parent_eof_stops_workers_disables_lease_and_does_not_accept_more_requests() {
    let (_temp, web, _backup, mut session) = fixture();
    install(&mut session);
    session
        .dispatch(request(
            3,
            Command::Start {
                session: "parent-session".into(),
            },
        ))
        .unwrap();
    let mut output = vec![];
    serve(&mut session, &mut Cursor::new([]), &mut output).unwrap();
    let runtime = web.join("technical-specs-runtime.json");
    let stopped = fs::read(&runtime).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&stopped).unwrap()["enabled"],
        false
    );
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert_eq!(fs::read(&runtime).unwrap(), stopped);
    assert!(session.dispatch(request(4, Command::Status {})).is_err());
}
#[test]
fn malformed_parent_request_and_failed_reply_delivery_also_stop_active_service() {
    for broken_output in [false, true] {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (_temp, web, _backup, mut session) = fixture();
        install(&mut session);
        session
            .dispatch(request(
                3,
                Command::Start {
                    session: "parent-session".into(),
                },
            ))
            .unwrap();
        let mut input = vec![];
        if broken_output {
            write_frame(&mut input, &request(4, Command::Status {})).unwrap();
            assert!(serve(&mut session, &mut Cursor::new(input), &mut Broken).is_err());
        } else {
            write_frame(&mut input, &serde_json::json!({"unrecognized":true})).unwrap();
            assert!(serve(&mut session, &mut Cursor::new(input), &mut vec![]).is_err());
        }
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &fs::read(web.join("technical-specs-runtime.json")).unwrap()
            )
            .unwrap()["enabled"],
            false
        );
    }
}
#[test]
fn live_service_blocks_maintenance_until_explicit_stop_and_remove_restores_html() {
    let (_temp, web, _backup, mut session) = fixture();
    let original = fs::read(web.join("index.html")).unwrap();
    install(&mut session);
    session
        .dispatch(request(
            3,
            Command::Start {
                session: "active".into(),
            },
        ))
        .unwrap();
    let refused = session
        .dispatch(request(
            4,
            Command::Plan {
                id: "remove".into(),
                action: Action::Remove,
                index: index(),
            },
        ))
        .unwrap();
    let Outcome::Failed(error) = refused.outcome else {
        panic!("maintenance was allowed while active")
    };
    assert_eq!(error.code, "emby-stop-required");
    session.dispatch(request(5, Command::Stop {})).unwrap();
    let Outcome::Plan(plan) = session
        .dispatch(request(
            6,
            Command::Plan {
                id: "remove".into(),
                action: Action::Remove,
                index: index(),
            },
        ))
        .unwrap()
        .outcome
    else {
        panic!("plan failed")
    };
    session
        .dispatch(request(
            7,
            Command::Apply {
                id: plan.id,
                fingerprint: plan.fingerprint,
            },
        ))
        .unwrap();
    assert_eq!(fs::read(web.join("index.html")).unwrap(), original);
    assert!(!web.join("technical-specs-card.js").exists());
    assert!(!web.join("technical-specs-runtime.json").exists());
}

#[test]
fn actual_channel_disconnect_stops_service_and_receipt_survives_reconnect() {
    use std::{
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    // Loopback is only this test's duplex transport, never a privileged endpoint.
    let (_temp, web, backup, session) = fixture();
    drop(session);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let worker_web = web.clone();
    let worker_backup = backup.clone();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut session = Session::open(&worker_web, &worker_backup).unwrap();
        serve(&mut session, &mut socket.try_clone().unwrap(), &mut socket)
    });
    let mut client = Client::from_authenticated_transport(socket);
    let Outcome::Plan(plan) = client
        .request(Command::Plan {
            id: "wire-install".into(),
            action: Action::Install,
            index: index(),
        })
        .unwrap()
    else {
        panic!("plan")
    };
    assert!(matches!(
        client
            .request(Command::Apply {
                id: plan.id.clone(),
                fingerprint: plan.fingerprint.clone()
            })
            .unwrap(),
        Outcome::Applied(_)
    ));
    assert!(matches!(
        client
            .request(Command::Start {
                session: "wire-session".into()
            })
            .unwrap(),
        Outcome::Service(_)
    ));
    drop(client);
    worker.join().unwrap().unwrap();
    let runtime: serde_json::Value =
        serde_json::from_slice(&fs::read(web.join("technical-specs-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(runtime["enabled"], false);
    let mut restored = Session::open(&web, &backup).unwrap();
    let Outcome::Operation(receipt) = restored
        .dispatch(request(1, Command::Operation { id: plan.id }))
        .unwrap()
        .outcome
    else {
        panic!("receipt")
    };
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.fingerprint, plan.fingerprint);
}
#[test]
fn lost_apply_reply_is_not_retried_and_committed_receipt_can_be_queried() {
    use std::{
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    let (_temp, web, backup, session) = fixture();
    drop(session);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let worker_web = web.clone();
    let worker_backup = backup.clone();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut session = Session::open(&worker_web, &worker_backup).unwrap();
        let plan: Request =
            serde_json::from_slice(&read_frame(&mut socket).unwrap().unwrap()).unwrap();
        write_frame(&mut socket, &session.dispatch(plan).unwrap()).unwrap();
        let apply: Request =
            serde_json::from_slice(&read_frame(&mut socket).unwrap().unwrap()).unwrap();
        assert!(matches!(
            session.dispatch(apply).unwrap().outcome,
            Outcome::Applied(_)
        ));
        // Commit succeeded, but the response is lost before the parent can know.
    });
    let mut client = Client::from_authenticated_transport(socket);
    let Outcome::Plan(plan) = client
        .request(Command::Plan {
            id: "lost-reply".into(),
            action: Action::Install,
            index: index(),
        })
        .unwrap()
    else {
        panic!("plan")
    };
    assert_eq!(
        client
            .request(Command::Apply {
                id: plan.id.clone(),
                fingerprint: plan.fingerprint
            })
            .unwrap_err()
            .code,
        "maintenance-result-unverified"
    );
    assert!(client.request(Command::Status {}).is_err());
    worker.join().unwrap();
    let mut restored = Session::open(&web, &backup).unwrap();
    let Outcome::Operation(receipt) = restored
        .dispatch(request(1, Command::Operation { id: plan.id }))
        .unwrap()
        .outcome
    else {
        panic!("receipt")
    };
    assert_eq!(receipt.phase, "committed");
}
#[test]
fn mismatched_responses_close_channel_and_do_not_send_a_second_request() {
    use std::{
        cell::RefCell,
        io::{Read, Write},
        rc::Rc,
    };
    struct Wire {
        input: Cursor<Vec<u8>>,
        written: Rc<RefCell<Vec<u8>>>,
    }
    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }
    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for reply in [
        Reply {
            version: 2,
            sequence: 1,
            outcome: Outcome::Closed,
        },
        Reply {
            version: 1,
            sequence: 2,
            outcome: Outcome::Closed,
        },
        Reply {
            version: 1,
            sequence: 1,
            outcome: Outcome::Published(true),
        },
    ] {
        let mut input = vec![];
        write_frame(&mut input, &reply).unwrap();
        let written = Rc::new(RefCell::new(vec![]));
        let mut client = Client::from_authenticated_transport(Wire {
            input: Cursor::new(input),
            written: written.clone(),
        });
        assert_eq!(
            client.request(Command::Close {}).unwrap_err().code,
            "maintenance-result-unverified"
        );
        let first = written.borrow().clone();
        assert!(client.request(Command::Close {}).is_err());
        assert_eq!(*written.borrow(), first);
    }
}

#[test]
fn helper_web_repair_keeps_running_session_and_does_not_publish_a_new_index() {
    let (_temp, web, _backup, mut session) = fixture();
    install(&mut session);
    let started = session
        .dispatch(request(
            3,
            Command::Start {
                session: "same-session".into(),
            },
        ))
        .unwrap();
    let Outcome::Service(started) = started.outcome else {
        panic!("start failed")
    };
    assert_eq!(started.phase, "starting");
    assert!(started.last_started_at.is_none());
    let Outcome::Service(started) = session
        .dispatch(request(
            4,
            Command::IndexCurrent {
                fingerprint: Some(emby::index_fingerprint(&index()).unwrap()),
            },
        ))
        .unwrap()
        .outcome
    else {
        panic!("enable failed")
    };
    assert_eq!(started.phase, "running");
    assert!(started.last_started_at.is_some());
    let data = fs::read(web.join("technical-specs-data.json")).unwrap();
    let nfo = fs::read(web.join("untouched.nfo")).unwrap();
    let repair = request(
        5,
        Command::Repair {
            id: "repair-web".into(),
        },
    );
    let repaired = session.dispatch(repair.clone()).unwrap();
    assert!(matches!(repaired.outcome,Outcome::Applied(ref value) if value.healthy));
    assert!(matches!(
        session.dispatch(repair).unwrap().outcome,
        Outcome::Applied(_)
    ));
    let status = session.dispatch(request(6, Command::Status {})).unwrap();
    let Outcome::Status { service, .. } = status.outcome else {
        panic!("status failed")
    };
    assert_eq!(service.phase, "running");
    assert_eq!(service.last_started_at, started.last_started_at);
    assert_eq!(
        service.lease.unwrap().session_id,
        started.lease.unwrap().session_id
    );
    assert_eq!(
        fs::read(web.join("technical-specs-data.json")).unwrap(),
        data
    );
    assert_eq!(fs::read(web.join("untouched.nfo")).unwrap(), nfo);
    let receipt = session
        .dispatch(request(
            7,
            Command::Operation {
                id: "repair-web".into(),
            },
        ))
        .unwrap();
    assert!(
        matches!(receipt.outcome,Outcome::Operation(ref plan) if plan.phase=="committed"&&plan.action=="repair-web")
    );
    session.dispatch(request(8, Command::Close {})).unwrap();
}
