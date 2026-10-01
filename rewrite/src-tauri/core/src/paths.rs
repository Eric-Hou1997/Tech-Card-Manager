use crate::{AppError, Result};
use std::path::{Component, Path, PathBuf};

// Platform path policy lives here; callers never authorize a path using a string prefix.
pub fn checked(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::new("invalid-path", "Absolute path required").at(path.display()));
    }
    let mut walked = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir) {
            return Err(
                AppError::new("ambiguous-path", "Parent traversal rejected").at(path.display())
            );
        }
        walked.push(part);
        // A Windows drive/UNC prefix is not a filesystem object until RootDir.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&walked)
            .map_err(|e| AppError::new("path-unavailable", e).at(walked.display()))?;
        if meta.file_type().is_symlink() {
            return Err(AppError::new(
                "ambiguous-path",
                "Symbolic links require an explicit real root",
            )
            .at(walked.display()));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(
                    AppError::new("ambiguous-path", "Reparse point rejected").at(walked.display())
                );
            }
        }
    }
    path.canonicalize()
        .map_err(|e| AppError::new("path-unavailable", e).at(path.display()))
}
pub fn within(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = checked(root)?;
    let path = checked(path)?;
    if !path.starts_with(root) {
        return Err(
            AppError::new("outside-root", "Path is outside configured root").at(path.display()),
        );
    }
    Ok(path)
}

/// Saving a directory is not proof that it is readable. Preserve offline
/// selections while still checking every reachable ancestor; actual reads
/// must continue through `checked`/`within` after the directory returns.
pub(crate) fn configured_directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::new("invalid-path", "Absolute path required").at(path.display()));
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(AppError::new("ambiguous-path", "Parent traversal rejected").at(path.display()));
    }
    let clean: PathBuf = path.components().collect();
    let share = clean.components().any(|part| matches!(part, Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..))));
    if !share
        && !clean
            .components()
            .any(|part| matches!(part, Component::Normal(_)))
    {
        return Err(AppError::new("invalid-root", "不能直接扫描整个磁盘根目录").at(path.display()));
    }
    let mut ancestor = clean.as_path();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let real = checked(ancestor)?;
                if !real.is_dir() {
                    return Err(
                        AppError::new("invalid-root", "Library root must be a directory")
                            .at(ancestor.display()),
                    );
                }
                let tail = clean
                    .strip_prefix(ancestor)
                    .map_err(|e| AppError::new("invalid-path", e))?;
                return Ok(if tail.as_os_str().is_empty() {
                    real
                } else {
                    real.join(tail)
                });
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                match ancestor.parent() {
                    Some(parent) if parent != ancestor => ancestor = parent,
                    // An unavailable drive/share has no reachable ancestor.
                    // Keep its absolute spelling; the reader will fail closed.
                    _ => return Ok(clean),
                }
            }
            Err(error) => {
                return Err(AppError::new("path-unavailable", error).at(ancestor.display()))
            }
        }
    }
}
