use super::*;
use product_core::{emby, maintenance::Client, maintenance_pipe::Pipe};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command as Process, Stdio},
    sync::{mpsc, Arc},
    thread::JoinHandle,
    time::{Duration, Instant},
};
const IO: Duration = Duration::from_secs(10);
struct Duplex {
    input: Pipe,
    output: Pipe,
}
impl Read for Duplex {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(out)
    }
}
impl Write for Duplex {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.output.write(data)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}
fn request(client: &mut Client<Duplex>, command: Command) -> Result<Outcome> {
    if let Some(pipe) = client.transport_mut() {
        pipe.input.reset(IO);
        pipe.output.reset(IO);
    }
    match client.request(command)? {
        Outcome::Failed(error) => Err(error),
        outcome => Ok(outcome),
    }
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
fn wait_child(child: &mut Child, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e|AppError::new("maintenance-process",e))? {
        Some(status) if status.success()=>return Ok(()),
        Some(status)=>return Err(AppError::new(match status.code(){Some(126)=>"maintenance-authorization-cancelled",Some(127)=>"maintenance-authorization-denied",_=>"maintenance-helper-failed"},format!("Permission helper exited with {status}"))),
        None if Instant::now()>=deadline=>return Err(AppError::new("maintenance-cleanup-unverified","Helper exit was not confirmed; the closed pipe prevents further requests and its idle deadline stops lease renewal")),
        None=>std::thread::sleep(Duration::from_millis(20)),
    }
    }
}
struct ProcessOwner {
    client: Option<Client<Duplex>>,
    child: Child,
}
impl ProcessOwner {
    fn finish(&mut self) -> Result<()> {
        let closing = match self.client.as_mut() {
            Some(client) => request(client, Command::Close {}).map(|_| ()),
            None => Ok(()),
        };
        self.client = None;
        let stopped = wait_child(&mut self.child, Duration::from_secs(17));
        match (closing, stopped) {
            (_, Err(e)) | (Err(e), Ok(())) => Err(e),
            _ => Ok(()),
        }
    }
}
impl Drop for ProcessOwner {
    fn drop(&mut self) {
        self.client = None;
        self.child.stdin = None;
        self.child.stdout = None;
        if let Err(error) = wait_child(&mut self.child, Duration::from_secs(17)) {
            eprintln!("maintenance-cleanup: {error}");
        }
    }
}
pub fn launch(web: &Path, store: Arc<product_core::store::Store>) -> Result<Connection> {
    let app = std::env::current_exe().map_err(|e| AppError::new("maintenance-executable", e))?;
    let helper = app
        .parent()
        .ok_or_else(|| AppError::new("maintenance-executable", "Application parent missing"))?
        .join("tcm-maintenance-helper");
    if !helper.is_file() {
        return Err(AppError::new(
            "maintenance-helper-missing",
            "Reinstall this package; its native permission helper is missing",
        ));
    }
    let child=Process::new("/usr/bin/pkexec").arg("--disable-internal-agent").arg(&helper).arg("--serve").arg(std::process::id().to_string()).arg(web).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e|AppError::new("maintenance-authorization-unavailable",format!("System authorization could not start: {e}; install polkit and a desktop authentication agent")))?;
    let mut owner = ProcessOwner {
        client: None,
        child,
    };
    // Only this process has the inherited endpoints. pkexec displays the exact
    // packaged executable and retains its standard administrator authorization.
    let input = Pipe::new(
        owner.child.stdout.take().unwrap().into(),
        Duration::from_secs(90),
    );
    let output = Pipe::new(
        owner.child.stdin.take().unwrap().into(),
        Duration::from_secs(90),
    );
    let (input, output) = match (input, output) {
        (Ok(i), Ok(o)) => (i, o),
        _ => {
            let _ = wait_child(&mut owner.child, Duration::from_secs(17));
            return Err(AppError::new(
                "maintenance-pipe",
                "Could not establish private authorization pipes",
            ));
        }
    };
    let mut client = Client::from_authenticated_transport(Duplex { input, output });
    // The first read includes the interactive system authorization, before the
    // normal ten-second request deadline becomes active.
    let hello = client.request(Command::Status {});
    if !matches!(hello, Ok(Outcome::Status { .. })) {
        drop(client);
        let stopped = wait_child(&mut owner.child, Duration::from_secs(17));
        return Err(match (hello, stopped) {
            (_, Err(e)) if e.code == "maintenance-cleanup-unverified" => e,
            (Ok(Outcome::Failed(e)), _) => e,
            (_, Err(e)) => e,
            (Err(e), Ok(())) => e,
            _ => AppError::new("maintenance-protocol", "Invalid authorization reply"),
        });
    }
    owner.client = Some(client);
    let (tx, rx) = mpsc::channel::<Work>();
    let worker = std::thread::Builder::new()
        .name("emby-permission".into())
        .spawn(move || {
            let mut revision = None;
            let mut next_tick = Instant::now() + Duration::from_secs(2);
            let mut publish_error: Option<AppError> = None;
            loop {
                let client = owner.client.as_mut().expect("owned maintenance client");
                match rx.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
                    Ok(work) => {
                        let start = matches!(work.command, Command::Start { .. });
                        if start {
                            revision = None;
                            publish_error = None;
                        }
                        let mut result = request(client, work.command);
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
                    let status = request(client, Command::Status {});
                    match status {
                        Ok(Outcome::Status { service, .. }) if service.phase == "running" => {
                            let publication = (|| -> Result<()> {
                                if let Some((next, items)) = store.publication_snapshot(revision)? {
                                    request(
                                        client,
                                        Command::Publish {
                                            index: emby::public_index(&items, emby::timestamp()),
                                        },
                                    )?;
                                    revision = Some(next);
                                }
                                Ok(())
                            })();
                            if let Err(error) = publication {
                                publish_error = Some(error);
                                let _ = request(client, Command::Stop {});
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
    #[test]
    fn system_authorization_cancel_and_denial_are_distinct() {
        for (code, expected) in [
            (126, "maintenance-authorization-cancelled"),
            (127, "maintenance-authorization-denied"),
        ] {
            let mut child = Process::new("/bin/sh")
                .args(["-c", &format!("exit {code}")])
                .spawn()
                .unwrap();
            assert_eq!(
                wait_child(&mut child, Duration::from_secs(2))
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }
    #[test]
    fn failed_setup_owner_closes_inherited_input_and_waits_for_child() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("exited");
        let child = Process::new("/bin/sh")
            .args(["-c", "cat >/dev/null; printf exited > \"$1\"", "fixture"])
            .arg(&marker)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        drop(ProcessOwner {
            client: None,
            child,
        });
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(std::fs::read(marker).unwrap(), b"exited");
    }
}
