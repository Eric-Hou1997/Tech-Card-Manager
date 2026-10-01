//! Dedicated Windows UAC helper. No Manager database, media roots or arbitrary
//! commands are accepted; the authenticated protocol owns one protected target.
use crate::{
    maintenance::{self, Session},
    windows_security::{self, Policy},
    windows_transport, AppError, Result,
};
use std::{ffi::OsString, io::Write, os::windows::io::AsRawHandle, path::PathBuf, time::Duration};
use windows_sys::Win32::{Foundation::WAIT_TIMEOUT, System::Threading::WaitForSingleObject};
fn denied(error: impl ToString) -> AppError {
    AppError::new("maintenance-untrusted-path", error)
}
fn target(policy: &Policy, web: &std::path::Path) -> Result<()> {
    // web and every ancestor are pinned for the entire session by initialize.
    policy.object(web, true, false, false).map_err(denied)?;
    for name in [
        "index.html",
        "technical-specs-card.js",
        "technical-specs-data.json",
        "technical-specs-languages.json",
        "technical-specs-runtime.json",
        ".tcm-web.lock",
    ] {
        let path = web.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                policy
                    .object(&path, false, false, false)
                    .map_err(|e| denied(e).at(path.display()))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(denied(e).at(path.display())),
        }
    }
    Ok(())
}
fn execute(args: &[OsString]) -> Result<()> {
    if args.len() != 4 || args[0] != "--serve" {
        return Err(AppError::new(
            "maintenance-authorization-required",
            "Launch maintenance from its packaged Manager",
        ));
    }
    windows_security::elevated()
        .map_err(|e| AppError::new("maintenance-authorization-required", e))?;
    let parent = args[1]
        .to_str()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&p| p > 1)
        .ok_or_else(|| denied("Invalid Manager process"))?;
    let process = windows_security::parent_process(parent).map_err(denied)?;
    let endpoint = args[2]
        .to_str()
        .ok_or_else(|| denied("Invalid maintenance endpoint"))?;
    let mut pipe = windows_transport::connect(endpoint, parent, Duration::from_secs(15))
        .map_err(|e| AppError::new("maintenance-pipe", e))?;
    let mut session = match (|| -> Result<_> {
        windows_security::prepare_helper_owner().map_err(denied)?;
        let policy = Policy::new().map_err(denied)?;
        let web = PathBuf::from(&args[3]);
        let held_web = policy
            .tree(&web, false)
            .map_err(|e| denied(e).at(web.display()))?;
        target(&policy, &web)?;
        let (journal, held_journal) = policy.journal().map_err(denied)?;
        let session = Session::open(&web, &journal)?;
        Ok((session, policy, web, journal, held_web, held_journal))
    })() {
        Ok(value) => value,
        Err(error) => {
            // Initialization failure still returns the structured first reply.
            let _ = maintenance::write_frame(
                &mut pipe,
                &maintenance::Reply {
                    version: 1,
                    sequence: 1,
                    outcome: maintenance::Outcome::Failed(error.clone()),
                },
            );
            return Err(error);
        }
    };
    let result = (|| {
        loop {
            pipe.reset(Duration::from_secs(15));
            let Some(bytes) = maintenance::read_frame(&mut pipe)? else {
                break;
            };
            if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
                return Err(AppError::new("maintenance-disconnected", "Manager exited"));
            }
            target(&session.1, &session.2)?;
            // The initial recursive check plus the pinned private root excludes
            // unprivileged recovery-file changes. Do not rescan all historical
            // backups on each two-second status/lease query.
            session
                .1
                .object(&session.3, true, false, true)
                .map_err(denied)?;
            let request: maintenance::Request = serde_json::from_slice(&bytes)?;
            let closing = matches!(request.request, maintenance::Command::Close {});
            let reply = session.0.dispatch(request)?;
            target(&session.1, &session.2)?;
            session
                .1
                .object(&session.3, true, false, true)
                .map_err(denied)?;
            pipe.reset(Duration::from_secs(15));
            maintenance::write_frame(&mut pipe, &reply)?;
            if closing {
                break;
            }
        }
        Ok(())
    })();
    let stopped = session.0.stop();
    match (result, stopped) {
        (Err(error), Err(cleanup)) => Err(AppError::new(
            "maintenance-cleanup",
            format!("{error}; {cleanup}"),
        )),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}
pub fn main() -> i32 {
    match execute(&std::env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            74
        }
    }
}
