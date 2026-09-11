//! Local application replacement has its own recovery journal, separate from
//! data migration. The old installation is retained until next-start health checks.
use crate::{hash, paths, AppError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replacement {
    pub operation_id: String,
    pub target: PathBuf,
    pub backup: PathBuf,
    pub candidate: PathBuf,
    pub before: String,
    pub after: String,
    pub phase: String,
}
fn io(e: std::io::Error) -> AppError {
    AppError::new("update-filesystem", e)
}
fn tree(path: &Path, root: &Path, entries: &mut Vec<String>) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(io)?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::new(
            "update-ambiguous-path",
            "Symlink installation entries require explicit bundle adaptation",
        )
        .at(path.display()));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|e| AppError::new("update-path", e))?
        .to_string_lossy();
    if metadata.is_dir() {
        entries.push(format!("d:{relative}"));
        let mut children = fs::read_dir(path)
            .map_err(io)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(io)?;
        children.sort();
        for child in children {
            tree(&child, root, entries)?;
        }
    } else if metadata.is_file() {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        let mut file = File::open(path).map_err(io)?;
        let mut block = [0u8; 65536];
        loop {
            let n = file.read(&mut block).map_err(io)?;
            if n == 0 {
                break;
            }
            digest.update(&block[..n]);
        }
        entries.push(format!("f:{relative}:{:x}", digest.finalize()));
    } else {
        return Err(
            AppError::new("update-file-kind", "Unsupported installation entry").at(path.display()),
        );
    }
    Ok(())
}
pub fn tree_hash(path: &Path) -> Result<String> {
    let mut entries = Vec::new();
    tree(path, path, &mut entries)?;
    Ok(hash(&serde_json::to_vec(&entries)?))
}
fn sync(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path).and_then(|f| f.sync_all()).map_err(io)?;
    #[cfg(windows)]
    let _ = path;
    Ok(())
}
fn save(path: &Path, value: &Replacement) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("update-path", "No journal parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    temporary
        .write_all(&serde_json::to_vec(value)?)
        .map_err(io)?;
    temporary.as_file().sync_all().map_err(io)?;
    temporary.persist(path).map_err(|e| io(e.error))?;
    sync(parent)
}
pub fn stage(
    journal: &Path,
    id: &str,
    target: &Path,
    artifact: &[u8],
    channel: &str,
) -> Result<Replacement> {
    if id.is_empty() || id.len() > 96 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::new(
            "invalid-operation-id",
            "Invalid update operation ID",
        ));
    }
    if !matches!(channel, "app" | "appimage") {
        return Err(AppError::new(
            "update-channel",
            "This replacement adapter owns app/AppImage only",
        ));
    }
    let target = paths::checked(target)?;
    let parent = target
        .parent()
        .ok_or_else(|| AppError::new("update-path", "Missing installation parent"))?;
    let work = parent.join(format!(".application-update-{id}"));
    if work.exists() {
        return Err(AppError::new(
            "update-operation-exists",
            "Recover the previous update before staging again",
        ));
    }
    fs::create_dir(&work).map_err(io)?;
    let candidate = work.join("candidate");
    let backup = work.join("previous");
    let staged = (|| {
        if channel == "app" {
            fs::create_dir(&candidate).map_err(io)?;
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(artifact));
            let mut prefix: Option<String> = None;
            let mut expanded = 0u64;
            for entry in archive.entries().map_err(io)? {
                let mut entry = entry.map_err(io)?;
                let path = entry.path().map_err(io)?.into_owned();
                if path
                    .components()
                    .any(|p| !matches!(p, Component::Normal(_)))
                {
                    return Err(AppError::new(
                        "update-archive-path",
                        "Non-relative archive entry",
                    ));
                }
                let mut parts = path.components();
                let top = parts
                    .next()
                    .ok_or_else(|| AppError::new("update-archive-path", "Empty archive entry"))?
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned();
                if !top.ends_with(".app") || prefix.as_ref().is_some_and(|p| p != &top) {
                    return Err(AppError::new(
                        "update-archive-layout",
                        "Expected one application bundle",
                    ));
                }
                prefix = Some(top);
                let relative: PathBuf = parts.collect();
                if relative.as_os_str().is_empty() {
                    continue;
                }
                let kind = entry.header().entry_type();
                if !kind.is_file() && !kind.is_dir() {
                    return Err(AppError::new(
                        "update-archive-kind",
                        "Links and special entries are not allowed in this application bundle",
                    ));
                }
                expanded = expanded
                    .checked_add(entry.size())
                    .ok_or_else(|| AppError::new("update-archive-size", "Archive size overflow"))?;
                if expanded > 4 * 1024 * 1024 * 1024 {
                    return Err(AppError::new(
                        "update-archive-size",
                        "Expanded application exceeds 4 GiB",
                    ));
                }
                let output = candidate.join(relative);
                if let Some(dir) = output.parent() {
                    fs::create_dir_all(dir).map_err(io)?;
                }
                entry.unpack(&output).map_err(io)?;
                if output.is_file() {
                    File::open(output).and_then(|f| f.sync_all()).map_err(io)?;
                }
            }
            if !candidate.join("Contents/Info.plist").is_file()
                || !candidate.join("Contents/MacOS").is_dir()
            {
                return Err(AppError::new(
                    "update-archive-layout",
                    "Application bundle is incomplete",
                ));
            }
        } else {
            let mut file = File::create(&candidate).map_err(io)?;
            file.write_all(artifact).map_err(io)?;
            file.set_permissions(fs::metadata(&target).map_err(io)?.permissions())
                .map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        sync(&work)?;
        let before = tree_hash(&target)?;
        let replacement = Replacement {
            operation_id: id.into(),
            target,
            backup,
            candidate,
            before,
            after: String::new(),
            phase: "staged".into(),
        };
        Ok(replacement)
    })();
    match staged {
        Ok(mut value) => {
            value.after = tree_hash(&value.candidate)?;
            save(journal, &value)?;
            Ok(value)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&work);
            Err(error)
        }
    }
}
pub fn commit(journal: &Path) -> Result<Replacement> {
    let mut value: Replacement = serde_json::from_slice(&fs::read(journal).map_err(io)?)?;
    if value.phase != "staged" {
        return recover(journal);
    }
    validate(&value)?;
    if tree_hash(&value.target)? != value.before || tree_hash(&value.candidate)? != value.after {
        return Err(AppError::new(
            "update-source-changed",
            "Application changed after preparation",
        ));
    }
    value.phase = "replacing".into();
    save(journal, &value)?;
    if value.target.is_dir() {
        swap_bundle(&value.target, &value.candidate)?;
        sync(value.target.parent().unwrap())?;
        // A crash here leaves the previous bundle in candidate; recover recognizes it.
        fs::rename(&value.candidate, &value.backup).map_err(io)?;
    } else {
        // Retain the old inode before the single atomic rename. No absent target interval.
        fs::hard_link(&value.target, &value.backup).map_err(io)?;
        sync(value.backup.parent().unwrap())?;
        fs::rename(&value.candidate, &value.target).map_err(io)?;
    }
    sync(value.target.parent().unwrap())?;
    sync(value.backup.parent().unwrap())?;
    value.phase = "installed-awaiting-health".into();
    save(journal, &value)?;
    Ok(value)
}
#[cfg(target_os = "macos")]
fn swap_bundle(a: &Path, b: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renamex_np(a: *const std::ffi::c_char, b: *const std::ffi::c_char, flags: u32) -> i32;
    }
    let a = CString::new(a.as_os_str().as_bytes()).map_err(|e| AppError::new("update-path", e))?;
    let b = CString::new(b.as_os_str().as_bytes()).map_err(|e| AppError::new("update-path", e))?;
    // Darwin sys/stdio.h RENAME_SWAP: exchange both existing directory entries atomically.
    if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), 2) } != 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn swap_bundle(_: &Path, _: &Path) -> Result<()> {
    Err(AppError::new(
        "update-platform",
        "Application bundle replacement requires macOS",
    ))
}
fn validate(value: &Replacement) -> Result<()> {
    if value.operation_id.is_empty()
        || value.operation_id.len() > 96
        || !value
            .operation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::new(
            "update-journal",
            "Invalid operation identifier",
        ));
    }
    let parent = value
        .target
        .parent()
        .ok_or_else(|| AppError::new("update-journal", "Missing target parent"))?;
    paths::checked(parent)?;
    let work = parent.join(format!(".application-update-{}", value.operation_id));
    paths::checked(&work)?;
    if value.backup != work.join("previous") || value.candidate != work.join("candidate") {
        return Err(AppError::new(
            "update-journal",
            "Recovery paths do not belong to the operation",
        ));
    }
    Ok(())
}
pub fn recover(journal: &Path) -> Result<Replacement> {
    let mut value: Replacement = serde_json::from_slice(&fs::read(journal).map_err(io)?)?;
    validate(&value)?;
    if value.phase == "rolling-back" {
        let actual = tree_hash(&value.target)?;
        if actual == value.before {
            value.phase = "rolled-back".into();
            save(journal, &value)?;
            return Ok(value);
        }
        if actual == value.after && tree_hash(&value.backup)? == value.before {
            if value.target.is_dir() {
                swap_bundle(&value.target, &value.backup)?;
            } else {
                fs::rename(&value.backup, &value.target).map_err(io)?;
            }
            sync(value.target.parent().unwrap())?;
            sync(value.backup.parent().unwrap())?;
            value.phase = "rolled-back".into();
            save(journal, &value)?;
            return Ok(value);
        }
        return Err(AppError::new(
            "update-rollback-conflict",
            "Installation changed during rollback",
        ));
    }
    if value.phase != "replacing" {
        return Ok(value);
    }
    if value.target.exists() {
        let actual = tree_hash(&value.target)?;
        if actual == value.after {
            if !value.backup.exists()
                && value.candidate.exists()
                && tree_hash(&value.candidate)? == value.before
            {
                fs::rename(&value.candidate, &value.backup).map_err(io)?;
                sync(value.backup.parent().unwrap())?;
            }
            if !value.backup.exists() || tree_hash(&value.backup)? != value.before {
                return Err(AppError::new(
                    "update-recovery-conflict",
                    "Previous installation backup is missing or changed",
                ));
            }
            value.phase = "installed-awaiting-health".into();
        } else if actual == value.before
            && (!value.backup.exists() || tree_hash(&value.backup)? == value.before)
        {
            value.phase = "rolled-back".into();
        } else {
            return Err(AppError::new(
                "update-recovery-conflict",
                "Installation changed outside the updater",
            ));
        }
    } else if value.backup.exists() && tree_hash(&value.backup)? == value.before {
        fs::rename(&value.backup, &value.target).map_err(io)?;
        sync(value.target.parent().unwrap())?;
        value.phase = "rolled-back".into();
    } else {
        return Err(AppError::new(
            "update-recovery-required",
            "No verified installation or backup available",
        ));
    }
    save(journal, &value)?;
    Ok(value)
}

/// Validate the staged package against the reviewed running application identity.
/// Signature verification precedes staging; this additionally rejects release mixups.
pub fn validate_candidate(
    value: &Replacement,
    bundle_id: &str,
    version: &str,
    arch: &str,
) -> Result<()> {
    validate(value)?;
    if tree_hash(&value.candidate)? != value.after {
        return Err(AppError::new(
            "update-candidate-changed",
            "Staged application changed",
        ));
    }
    if value.candidate.is_dir() {
        let plist = plist::Value::from_file(value.candidate.join("Contents/Info.plist"))
            .map_err(|e| AppError::new("update-bundle-metadata", e))?;
        let dictionary = plist.as_dictionary().ok_or_else(|| {
            AppError::new("update-bundle-metadata", "Expected a plist dictionary")
        })?;
        let text = |key: &str| dictionary.get(key).and_then(plist::Value::as_string);
        if text("CFBundleIdentifier") != Some(bundle_id)
            || text("CFBundleShortVersionString") != Some(version.trim_start_matches('v'))
        {
            return Err(AppError::new(
                "update-bundle-identity",
                "Bundle identity or version differs from the reviewed update",
            ));
        }
        let executable = text("CFBundleExecutable")
            .filter(|s| {
                !s.is_empty()
                    && Path::new(s)
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                    && Path::new(s).components().count() == 1
            })
            .ok_or_else(|| {
                AppError::new("update-bundle-executable", "Invalid bundle executable")
            })?;
        let mut file =
            File::open(value.candidate.join("Contents/MacOS").join(executable)).map_err(io)?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).map_err(io)?;
        if arch != "aarch64"
            || header[..4] != [0xcf, 0xfa, 0xed, 0xfe]
            || header[4..] != [0x0c, 0, 0, 1]
        {
            return Err(AppError::new(
                "update-architecture",
                "Expected native Apple Silicon executable",
            ));
        }
    } else {
        let mut file = File::open(&value.candidate).map_err(io)?;
        let mut header = [0u8; 64];
        file.read_exact(&mut header).map_err(io)?;
        let machine = u16::from_le_bytes([header[18], header[19]]);
        if header[..4] != *b"\x7fELF"
            || header[4] != 2
            || header[5] != 1
            || header[8..11] != *b"AI\x02"
            || !matches!((arch, machine), ("x86_64", 62) | ("aarch64", 183))
        {
            return Err(AppError::new(
                "update-architecture",
                "Expected a type 2 AppImage for the installed application architecture",
            ));
        }
    }
    Ok(())
}

pub fn rollback(journal: &Path) -> Result<Replacement> {
    let mut value = recover(journal)?;
    if value.phase == "rolled-back" {
        return Ok(value);
    }
    validate(&value)?;
    if value.phase != "installed-awaiting-health"
        || tree_hash(&value.target)? != value.after
        || tree_hash(&value.backup)? != value.before
    {
        return Err(AppError::new(
            "update-rollback-conflict",
            "Verified previous and current installations are required",
        ));
    }
    value.phase = "rolling-back".into();
    save(journal, &value)?;
    if value.target.is_dir() {
        swap_bundle(&value.target, &value.backup)?;
    } else {
        fs::rename(&value.backup, &value.target).map_err(io)?;
    }
    sync(value.target.parent().unwrap())?;
    sync(value.backup.parent().unwrap())?;
    value.phase = "rolled-back".into();
    save(journal, &value)?;
    Ok(value)
}
