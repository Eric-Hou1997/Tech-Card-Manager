use super::{
    worker::{self, Connection, Owner},
    *,
};
use product_core::maintenance::Client;
use std::{
    ffi::OsStr,
    fs,
    io::Read,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::Path,
    process::{Child, Command as Process, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

const IO: Duration = Duration::from_secs(10);

fn authorization_code(detail: &str) -> &'static str {
    if detail.contains("(-128)") {
        "maintenance-authorization-cancelled"
    } else if detail.contains("(-60005)") {
        "maintenance-authorization-denied"
    } else {
        "maintenance-helper-failed"
    }
}

fn authorization_error(child: &mut Child, status: std::process::ExitStatus) -> AppError {
    let mut detail = String::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(16 * 1024).read_to_string(&mut detail);
    }
    let detail = detail.trim();
    let code = authorization_code(detail);
    AppError::new(
        code,
        if detail.is_empty() {
            format!("macOS permission helper exited with {status}")
        } else {
            format!("macOS permission helper failed: {detail}")
        },
    )
}

fn shell_quote(value: &OsStr) -> Result<String> {
    let value = value.to_str().ok_or_else(|| {
        AppError::new(
            "maintenance-path",
            "macOS authorization requires Unicode application and Emby paths",
        )
    })?;
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn apple_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}

fn wait_child(child: &mut Child, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|error| AppError::new("maintenance-process", error))? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(authorization_error(child, status)),
            None if Instant::now() >= deadline => {
                return Err(AppError::new(
                    "maintenance-cleanup-unverified",
                    "Helper exit was not confirmed; the closed socket and idle deadline prevent further lease renewal",
                ))
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

struct PendingProcess(Option<Child>);
impl PendingProcess {
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("pending process must exist")
    }
    fn release(mut self) -> Child {
        self.0.take().expect("pending process must exist")
    }
}
impl Drop for PendingProcess {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct ProcessOwner {
    client: Option<Client<UnixStream>>,
    child: Child,
    _socket: tempfile::TempDir,
}
impl Owner for ProcessOwner {
    fn request(&mut self, command: Command) -> Result<Outcome> {
        let client = self.client.as_mut().ok_or_else(|| {
            AppError::new("maintenance-disconnected", "Permission session closed")
        })?;
        if let Some(stream) = client.transport_mut() {
            stream
                .set_read_timeout(Some(IO))
                .and_then(|()| stream.set_write_timeout(Some(IO)))
                .map_err(|error| AppError::new("maintenance-transport", error))?;
        }
        match client.request(command)? {
            Outcome::Failed(error) => Err(error),
            outcome => Ok(outcome),
        }
    }
    fn finish(&mut self) -> Result<()> {
        let closing = self.request(Command::Close {}).map(|_| ());
        self.client = None;
        let stopped = wait_child(&mut self.child, Duration::from_secs(17));
        match (closing, stopped) {
            (_, Err(error)) | (Err(error), Ok(())) => Err(error),
            _ => Ok(()),
        }
    }
}
impl Drop for ProcessOwner {
    fn drop(&mut self) {
        self.client = None;
        if let Err(error) = wait_child(&mut self.child, Duration::from_secs(17)) {
            eprintln!("maintenance-cleanup: {error}");
        }
    }
}

pub fn launch(web: &Path, store: Arc<product_core::store::Store>) -> Result<Connection> {
    let app =
        std::env::current_exe().map_err(|error| AppError::new("maintenance-executable", error))?;
    let helper = app
        .parent()
        .ok_or_else(|| AppError::new("maintenance-executable", "Application parent missing"))?
        .join("tcm-maintenance-helper");
    product_core::paths::checked(&helper).map_err(|error| {
        AppError::new(
            "maintenance-helper-missing",
            format!(
                "Reinstall this package; its permission helper is missing or ambiguous: {error}"
            ),
        )
    })?;
    let temporary = tempfile::Builder::new()
        .prefix("tcm-authorization-")
        .tempdir()
        .map_err(|error| AppError::new("maintenance-socket", error))?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::new("maintenance-socket", error))?;
    let socket = temporary.path().join("manager.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| AppError::new("maintenance-socket", error))?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::new("maintenance-socket", error))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| AppError::new("maintenance-socket", error))?;
    let command = format!(
        "exec {} --serve-macos {} {} {}",
        shell_quote(helper.as_os_str())?,
        std::process::id(),
        shell_quote(socket.as_os_str())?,
        shell_quote(web.as_os_str())?
    );
    let script = format!(
        "do shell script {} with administrator privileges",
        apple_string(&command)
    );
    let child = Process::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::new("maintenance-authorization-unavailable", error))?;
    let mut pending = PendingProcess(Some(child));
    let deadline = Instant::now() + Duration::from_secs(120);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = pending
                    .child()
                    .try_wait()
                    .map_err(|error| AppError::new("maintenance-process", error))?
                {
                    return Err(authorization_error(pending.child(), status));
                }
                if Instant::now() >= deadline {
                    return Err(AppError::new(
                        "maintenance-authorization-timeout",
                        "macOS authorization did not finish in time",
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(AppError::new("maintenance-socket", error)),
        }
    };
    let (uid, pid) = product_core::maintenance_macos::peer(&stream)?;
    let peer_path = product_core::maintenance_macos::process_path(pid)?
        .canonicalize()
        .map_err(|error| AppError::new("maintenance-untrusted-peer", error))?;
    if uid != 0
        || peer_path
            != helper
                .canonicalize()
                .map_err(|error| AppError::new("maintenance-untrusted-peer", error))?
    {
        return Err(AppError::new(
            "maintenance-untrusted-peer",
            "The authorized peer is not the packaged root helper",
        ));
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(90))))
        .map_err(|error| AppError::new("maintenance-transport", error))?;
    let mut owner = ProcessOwner {
        client: Some(Client::from_authenticated_transport(stream)),
        child: pending.release(),
        _socket: temporary,
    };
    match owner.request(Command::Status {})? {
        Outcome::Status { .. } => worker::launch(owner, store),
        _ => Err(AppError::new(
            "maintenance-protocol",
            "Invalid authorization reply",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_arguments_are_single_quoted_without_command_substitution() {
        assert_eq!(
            shell_quote(OsStr::new("a b'$(touch nope)")).unwrap(),
            "'a b'\\''$(touch nope)'"
        );
        let script = apple_string("exec 'a' \\ \"");
        assert_eq!(script, "\"exec 'a' \\\\ \\\"\"");
    }
    #[test]
    fn macos_authorization_result_keeps_cancel_denial_and_other_failures_distinct() {
        assert_eq!(
            authorization_code("User canceled. (-128)"),
            "maintenance-authorization-cancelled"
        );
        assert_eq!(
            authorization_code("Authorization denied. (-60005)"),
            "maintenance-authorization-denied"
        );
        assert_eq!(
            authorization_code("helper exit 74"),
            "maintenance-helper-failed"
        );
    }
    #[test]
    fn abandoned_authorization_process_is_terminated_and_reaped() {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        let child = Process::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        drop(PendingProcess(Some(child)));
        assert_eq!(unsafe { kill(pid, 0) }, -1);
    }
}
