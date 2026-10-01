//! Root helper entry for macOS. The GUI and helper communicate only through a
//! private Unix socket whose peer PID and effective UID are checked on both ends.
use crate::{
    maintenance::{self, Session},
    maintenance_linux::{journal, validate_target},
    AppError, Result,
};
use std::{
    ffi::{c_int, c_void, OsString},
    fs,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStringExt,
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::{Path, PathBuf},
    time::Duration,
};

const JOURNAL: &str = if crate::OFFICIAL_RELEASE {
    "/Library/Application Support/TechCardManager"
} else {
    "/Library/Application Support/TechCardManagerValidation"
};
const SOL_LOCAL: c_int = 0;
const LOCAL_PEERPID: c_int = 2;

#[link(name = "proc")]
extern "C" {
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
}

fn denied(message: impl ToString) -> AppError {
    AppError::new("maintenance-untrusted-peer", message)
}

pub fn process_path(pid: u32) -> Result<PathBuf> {
    let mut bytes = vec![0_u8; 4096];
    let length =
        unsafe { proc_pidpath(pid as c_int, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if length <= 0 {
        return Err(denied(std::io::Error::last_os_error()));
    }
    bytes.truncate(length as usize);
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

pub fn peer(stream: &UnixStream) -> Result<(u32, u32)> {
    let mut uid = 0;
    let mut gid = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(denied(std::io::Error::last_os_error()));
    }
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            SOL_LOCAL,
            LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        )
    } != 0
        || pid <= 1
        || size as usize != std::mem::size_of_val(&pid)
    {
        return Err(denied(std::io::Error::last_os_error()));
    }
    Ok((uid, pid as u32))
}

fn initialize(args: &[String]) -> Result<(Session, UnixStream, PathBuf)> {
    if args.len() != 4 || args[0] != "--serve-macos" || unsafe { libc::geteuid() } != 0 {
        return Err(AppError::new(
            "maintenance-authorization-required",
            "Launch the helper through the macOS administrator authorization flow",
        ));
    }
    let manager_pid = args[1].parse::<u32>().map_err(denied)?;
    let socket = Path::new(&args[2]);
    let own = std::env::current_exe()
        .map_err(denied)?
        .canonicalize()
        .map_err(denied)?;
    let manager = process_path(manager_pid)?.canonicalize().map_err(denied)?;
    if own.file_name().and_then(|value| value.to_str()) != Some("tcm-maintenance-helper")
        || own.parent() != manager.parent()
    {
        return Err(denied(
            "Helper must be launched beside its packaged Manager",
        ));
    }
    let socket_meta = fs::symlink_metadata(socket).map_err(denied)?;
    let parent = socket
        .parent()
        .ok_or_else(|| denied("Private socket parent is missing"))?;
    let parent_meta = fs::symlink_metadata(parent).map_err(denied)?;
    if !socket.is_absolute()
        || !socket_meta.file_type().is_socket()
        || socket_meta.uid() == 0
        || socket_meta.mode() & 0o077 != 0
        || parent_meta.uid() != socket_meta.uid()
        || !parent_meta.is_dir()
        || parent_meta.file_type().is_symlink()
        || parent_meta.mode() & 0o077 != 0
    {
        return Err(denied("Expected a private Manager-owned Unix socket"));
    }
    let stream = UnixStream::connect(socket).map_err(denied)?;
    let (manager_uid, peer_pid) = peer(&stream)?;
    if manager_uid != socket_meta.uid() || peer_pid != manager_pid {
        return Err(denied(
            "Manager socket identity does not match the authorized request",
        ));
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(15))))
        .map_err(denied)?;
    let web = Path::new(&args[3]);
    validate_target(web)?;
    let backup = journal(Path::new(JOURNAL), 0)?;
    unsafe { libc::umask(0o077) };
    Ok((Session::open(web, &backup)?, stream, web.to_owned()))
}

fn execute(args: &[String]) -> Result<()> {
    let (mut session, mut stream, web) = initialize(args)?;
    let mut output = stream.try_clone().map_err(denied)?;
    let result = (|| {
        while let Some(bytes) = maintenance::read_frame(&mut stream)? {
            validate_target(&web)?;
            let request: maintenance::Request = serde_json::from_slice(&bytes)?;
            let closing = matches!(request.request, maintenance::Command::Close {});
            let reply = session.dispatch(request)?;
            maintenance::write_frame(&mut output, &reply)?;
            if closing {
                break;
            }
        }
        Ok(())
    })();
    let stop = session.stop();
    match (result, stop) {
        (Err(error), Err(cleanup)) => Err(AppError::new(
            "maintenance-cleanup",
            format!("{error}; {cleanup}"),
        )),
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

pub fn main() -> i32 {
    match execute(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            74
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_unprivileged_launch_is_rejected() {
        let Err(error) = initialize(&[]) else {
            panic!("direct launch must fail")
        };
        assert_eq!(error.code, "maintenance-authorization-required");
    }
    #[test]
    fn unix_peer_reports_the_actual_process() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("peer.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        assert_eq!(peer(&client).unwrap().1, std::process::id());
        assert_eq!(peer(&server).unwrap().1, std::process::id());
        assert_eq!(peer(&client).unwrap().0, unsafe { libc::geteuid() });
    }
}
