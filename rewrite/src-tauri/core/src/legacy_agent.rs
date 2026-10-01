//! Fixed historical Agent files. Snapshots bind bytes to a file object, not a PID.
use crate::{hash, AppError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};
#[cfg(windows)]
pub mod windows;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub file: String,
    pub created: String,
    pub written: String,
    pub len: u64,
    pub attributes: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub path: String,
    pub identity: Identity,
    pub contents: Vec<u8>,
}
pub(crate) fn failure(path: &Path, message: impl std::fmt::Display) -> AppError {
    AppError::new("legacy-agent-unverified", message).at(path.display())
}
pub fn fixed_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || !matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("agent.pid" | "agent-heartbeat.txt")
        )
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            != Some("data")
    {
        return Err(failure(path, "旧版状态文件路径无效"));
    }
    Ok(())
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        let path = Path::new(&self.path);
        fixed_path(path)?;
        if self.contents.len() > 8192
            || self.identity.len != self.contents.len() as u64
            || [
                &self.identity.file,
                &self.identity.created,
                &self.identity.written,
            ]
            .iter()
            .any(|value| value.is_empty() || value.len() > 128)
        {
            return Err(failure(path, "旧版状态文件备份无效"));
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn archive_path(&self, operation: &str) -> Result<PathBuf> {
        self.validate()?;
        if operation.len() != 64 || !operation.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(failure(Path::new(&self.path), "旧版状态文件操作编号无效"));
        }
        Ok(Path::new(&self.path)
            .parent()
            .expect("fixed data directory")
            .join(format!(".tcm-agent-{operation}.bak")))
    }
}
fn read_snapshot(path: &Path, file: &mut File) -> Result<Snapshot> {
    let before = identity(file).map_err(|e| failure(path, e))?;
    if before.len > 8192 {
        return Err(failure(path, "旧版状态文件过大"));
    }
    let mut contents = Vec::new();
    (&mut *file)
        .take(8193)
        .read_to_end(&mut contents)
        .map_err(|e| failure(path, e))?;
    if identity(file).map_err(|e| failure(path, e))? != before
        || contents.len() as u64 != before.len
    {
        return Err(failure(path, "旧版状态文件在读取期间改变"));
    }
    Ok(Snapshot {
        path: path
            .to_str()
            .ok_or_else(|| failure(path, "路径编码有歧义"))?
            .into(),
        identity: before,
        contents,
    })
}
#[cfg(windows)]
pub fn snapshot(path: &Path) -> Result<Snapshot> {
    windows::snapshot(path)
}
#[cfg(unix)]
pub fn snapshot(path: &Path) -> Result<Snapshot> {
    use std::os::unix::fs::OpenOptionsExt;
    fixed_path(path)?;
    let path = crate::paths::checked(path)?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| failure(&path, e))?;
    let result = read_snapshot(&path, &mut file)?;
    result.validate()?;
    Ok(result)
}
#[cfg(unix)]
fn identity(file: &File) -> std::io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 {
        return Err(std::io::Error::other("状态文件不是独立普通文件"));
    }
    Ok(Identity {
        file: format!("{}:{}", m.dev(), m.ino()),
        created: format!("{}:{}", m.ctime(), m.ctime_nsec()),
        written: format!("{}:{}", m.mtime(), m.mtime_nsec()),
        len: m.len(),
        attributes: m.mode(),
    })
}
#[cfg(windows)]
use windows::identity;
