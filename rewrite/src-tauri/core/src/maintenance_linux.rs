//! Root helper entry for Linux. The protected journal location is compiled in;
//! unprivileged snapshots are never accepted as recovery instructions.
use crate::{
    maintenance::{self, Session},
    maintenance_pipe::Pipe,
    AppError, Result,
};
use std::{
    fs,
    io::Write,
    os::{
        fd::FromRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::Duration,
};
const JOURNAL: &str = "/var/lib/tech-card-manager-validation";
const FILES: &[&str] = &[
    "index.html",
    "technical-specs-card.js",
    "technical-specs-data.json",
    "technical-specs-languages.json",
    "technical-specs-runtime.json",
    ".tcm-web.lock",
];
fn denied(message: impl ToString) -> AppError {
    AppError::new("maintenance-untrusted-path", message)
}
fn metadata(path: &Path, directory: bool, uid: u32) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path).map_err(|e| denied(e).at(path.display()))?;
    if meta.uid() != uid
        || meta.mode() & 0o022 != 0
        || meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && (!meta.is_file() || meta.nlink() != 1))
    {
        return Err(denied(
            format!("Maintenance requires an owner-controlled directory or single-link regular file (expected uid {uid}, actual uid {}, mode {:o}, links {})", meta.uid(), meta.mode() & 0o7777, meta.nlink()),
        )
        .at(path.display()));
    }
    Ok(meta)
}
fn tree(path: &Path, uid: u32) -> Result<()> {
    if !path.is_absolute()
        || path.components().any(|p| {
            matches!(
                p,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(denied("Expected an absolute normalized path"));
    }
    for part in path.ancestors() {
        metadata(part, true, uid)?;
    }
    Ok(())
}
fn journal(root: &Path, uid: u32) -> Result<PathBuf> {
    let parent = root
        .parent()
        .ok_or_else(|| denied("Journal parent missing"))?;
    tree(parent, uid)?;
    match fs::create_dir(root) {
        Ok(()) => fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(denied)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(denied(e)),
    }
    metadata(root, true, uid)?;
    if fs::metadata(root).map_err(denied)?.mode() & 0o077 != 0 {
        return Err(denied(
            "Journal must remain private to the privileged helper",
        ));
    }
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).map_err(denied)? {
            let entry = entry.map_err(denied)?;
            let kind = entry.file_type().map_err(denied)?;
            metadata(&entry.path(), kind.is_dir(), uid)?;
            if kind.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    Ok(root.to_owned())
}
fn validate_target(web: &Path) -> Result<()> {
    tree(web, 0)?;
    for file in FILES {
        let target = web.join(file);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                metadata(&target, false, 0)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(denied(e)),
        }
    }
    Ok(())
}
fn initialize(args: &[String]) -> Result<(Session, PathBuf)> {
    if args.len() != 3 || args[0] != "--serve" || unsafe { libc::geteuid() } != 0 {
        return Err(AppError::new(
            "maintenance-authorization-required",
            "Launch the helper through the system authorization flow",
        ));
    }
    let parent = args[1].parse::<u32>().map_err(denied)?;
    let uid = std::env::var("PKEXEC_UID")
        .map_err(denied)?
        .parse::<u32>()
        .map_err(denied)?;
    if parent <= 1
        || unsafe { libc::getppid() } as u32 != parent
        || fs::metadata(format!("/proc/{parent}"))
            .map_err(denied)?
            .uid()
            != uid
    {
        return Err(denied(
            "System authorization parent identity does not match",
        ));
    }
    // Inherited descriptors are the sole transport; no socket, endpoint, token,
    // arbitrary backup directory, executable or shell arrives in the protocol.
    let own = std::env::current_exe()
        .map_err(denied)?
        .canonicalize()
        .map_err(denied)?;
    let manager = fs::read_link(format!("/proc/{parent}/exe")).map_err(denied)?;
    if manager.file_name().and_then(|s| s.to_str()) != Some("tcm-validation")
        || manager.parent() != own.parent()
    {
        return Err(denied("Helper must be launched by its packaged Manager"));
    }
    let web = Path::new(&args[2]);
    validate_target(web)?;
    let backup = journal(Path::new(JOURNAL), 0)?;
    // New files inherit a private mask; web assets explicitly retain their
    // original readable permissions through Integration's atomic writer.
    unsafe { libc::umask(0o077) };
    Ok((Session::open(web, &backup)?, web.to_owned()))
}
fn execute(args: &[String]) -> Result<()> {
    let (mut session, web) = match initialize(args) {
        Ok(value) => value,
        Err(error) => {
            let mut output = Pipe::new(
                unsafe { std::os::fd::OwnedFd::from_raw_fd(1) },
                Duration::from_secs(2),
            )
            .map_err(denied)?;
            let _ = maintenance::write_frame(
                &mut output,
                &maintenance::Reply {
                    version: 1,
                    sequence: 1,
                    outcome: maintenance::Outcome::Failed(error.clone()),
                },
            );
            return Err(error);
        }
    };
    let mut input = Pipe::new(
        unsafe { std::os::fd::OwnedFd::from_raw_fd(0) },
        Duration::from_secs(15),
    )
    .map_err(denied)?;
    let mut output = Pipe::new(
        unsafe { std::os::fd::OwnedFd::from_raw_fd(1) },
        Duration::from_secs(15),
    )
    .map_err(denied)?;
    let result = (|| {
        loop {
            input.reset(Duration::from_secs(15));
            let Some(bytes) = maintenance::read_frame(&mut input)? else {
                break;
            };
            // Recheck ownership before each operation, not only at launch.
            validate_target(&web)?;
            let request: maintenance::Request = serde_json::from_slice(&bytes)?;
            let closing = matches!(request.request, maintenance::Command::Close {});
            let reply = session.dispatch(request)?;
            output.reset(Duration::from_secs(15));
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
    fn helper_rejects_unprivileged_direct_launch() {
        assert!(initialize(&[]).is_err());
    }
    #[test]
    fn forged_recovery_files_and_links_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let uid = unsafe { libc::geteuid() };
        let file = root.join("operation.json");
        fs::write(&file, b"{}").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        metadata(&file, false, uid).unwrap();
        fs::hard_link(&file, root.join("duplicate")).unwrap();
        assert!(metadata(&file, false, uid).is_err());
        std::os::unix::fs::symlink(&file, root.join("linked")).unwrap();
        assert!(metadata(&root.join("linked"), false, uid).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(metadata(&root, true, uid).is_err());
    }
}
