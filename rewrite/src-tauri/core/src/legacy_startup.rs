//! Exact, recoverable snapshots for the two original current-user Run values.
use crate::{legacy_process::ExecutableEvidence, windows_arguments, AppError, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBackup {
    pub name: String,
    /// Original REG_SZ content without its single final NUL. Only the original
    /// fixed argument syntax is accepted; no arbitrary command is archived.
    pub command: Vec<u16>,
    pub executable: ExecutableEvidence,
}
impl RunBackup {
    pub fn path(&self) -> Result<String> {
        let path = match self.name.as_str() {
            "Tech Card Manager" => windows_arguments::original_login_executable(&self.command),
            "IMDb Tech Manager Agent" => {
                windows_arguments::original_agent_executable(&self.command)
            }
            _ => None,
        }
        .ok_or_else(|| {
            AppError::new(
                "legacy-startup-unowned",
                "旧登录项内容无法确认，已保留原记录",
            )
        })?;
        if !self.executable.baseline_assets_match
            || self.executable.path.is_empty()
            || self.executable.sha256.len() != 64
            || !self
                .executable
                .sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(AppError::new(
                "legacy-startup-unowned",
                "旧登录项程序来源无法确认",
            ));
        }
        String::from_utf16(&path).map_err(|e| AppError::new("legacy-startup-path", e))
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.path()?;
        Ok(crate::hash(&serde_json::to_vec(self)?))
    }
    pub fn matches_command(&self, current: Option<&[u16]>) -> Result<bool> {
        self.path()?;
        match current {
            None => Ok(false),
            Some(command) if command == self.command => Ok(true),
            Some(_) => Err(AppError::new(
                "legacy-startup-conflict",
                "旧登录项已改变，已保留当前记录",
            )),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Removal {
    Removed,
    AlreadyAbsent,
}

/// A backup alone does not prove that this operation removed a login entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunPhase {
    Removing,
    Removed,
    Absent,
    Restoring,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReceipt {
    pub id: String,
    pub backup: String,
    pub phase: RunPhase,
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn fixture() -> RunBackup {
        RunBackup {
            name: "Tech Card Manager".into(),
            command: r#""C:\旧版\Tech Card Manager.exe" --login-startup"#
                .encode_utf16()
                .collect(),
            executable: ExecutableEvidence {
                path: r"C:\旧版\Tech Card Manager.exe".into(),
                sha256: "a".repeat(64),
                baseline_assets_match: true,
            },
        }
    }
    #[test]
    fn snapshot_accepts_only_the_two_original_names_and_exact_argument_forms() {
        let original = fixture();
        assert_eq!(original.path().unwrap(), r"C:\旧版\Tech Card Manager.exe");
        assert!(original.matches_command(Some(&original.command)).unwrap());
        assert!(!original.matches_command(None).unwrap());
        assert_eq!(
            original
                .matches_command(Some(&"external command".encode_utf16().collect::<Vec<_>>()))
                .unwrap_err()
                .code,
            "legacy-startup-conflict"
        );
        for (name, command) in [
            ("other", r#""C:\old.exe" --login-startup"#),
            ("Tech Card Manager", r#""C:\old.exe" --agent"#),
            ("IMDb Tech Manager Agent", r#""C:\old.exe" --login-startup"#),
            (
                "Tech Card Manager",
                r#""C:\old.exe" --login-startup & other.exe"#,
            ),
        ] {
            let mut changed = original.clone();
            changed.name = name.into();
            changed.command = command.encode_utf16().collect();
            assert!(changed.path().is_err());
        }
        let mut agent = original.clone();
        agent.name = "IMDb Tech Manager Agent".into();
        agent.command = r#""C:\old.exe" --agent"#.encode_utf16().collect();
        assert_eq!(agent.path().unwrap(), r"C:\old.exe");
        let mut changed = original.clone();
        changed.executable.sha256 = "b".repeat(64);
        assert_ne!(
            changed.fingerprint().unwrap(),
            original.fingerprint().unwrap()
        );
        changed.executable.baseline_assets_match = false;
        assert!(changed.fingerprint().is_err());
    }
}
