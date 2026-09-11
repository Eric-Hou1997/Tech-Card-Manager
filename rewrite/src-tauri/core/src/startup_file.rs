//! File-backed login registration adapters; Windows uses the native registry adapter.
use crate::{paths, AppError, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
pub struct StartupFile {
    pub path: PathBuf,
    bytes: Vec<u8>,
    identity: String,
    os: String,
}
fn failure(error: impl ToString) -> AppError {
    AppError::new("autostart-file", error)
}
fn quoted(value: &str) -> Result<String> {
    if value.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
        return Err(failure(
            "Login path contains unsupported control characters",
        ));
    }
    // Desktop Entry general escaping is applied before Exec argument quoting.
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    ))
}
impl StartupFile {
    pub fn new(
        os: &str,
        directory: &Path,
        identity: &str,
        exe: &Path,
        display_name: &str,
    ) -> Result<Self> {
        if identity.is_empty()
            || !identity
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
        {
            return Err(failure("Invalid login registration identity"));
        }
        let exe = exe
            .to_str()
            .ok_or_else(|| failure("Executable path is not Unicode"))?;
        let (name,bytes)=match os {
            "macos"=>{
                let mut value=plist::Dictionary::new();value.insert("Label".into(),identity.into());value.insert("ProgramArguments".into(),plist::Value::Array(vec![exe.into(),"--background".into()]));value.insert("RunAtLoad".into(),true.into());
                let mut bytes=Vec::new();plist::Value::Dictionary(value).to_writer_xml(&mut bytes).map_err(failure)?;(format!("{identity}.plist"),bytes)
            },
            "linux"=>(format!("{identity}.desktop"),format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nExec={} --background\nTerminal=false\nStartupNotify=false\nX-TCM-ITM-Managed={identity}\n",display_name.replace('\\',"\\\\").replace(['\n','\r']," "),quoted(exe)?).into_bytes()),
            _=>return Err(failure("Unsupported file-backed login platform")),
        };
        Ok(Self {
            path: directory.join(name),
            bytes,
            identity: identity.into(),
            os: os.into(),
        })
    }
    fn existing(&self) -> Result<Option<Vec<u8>>> {
        match fs::symlink_metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(failure(e)),
            Ok(_) => {}
        }
        paths::checked(&self.path)?;
        let meta = fs::metadata(&self.path).map_err(failure)?;
        if !meta.is_file() || meta.len() > 1024 * 1024 {
            return Err(failure("Unexpected login registration file"));
        }
        let bytes = fs::read(&self.path).map_err(failure)?;
        let owned = if self.os == "macos" {
            plist::Value::from_reader(std::io::Cursor::new(&bytes))
                .ok()
                .and_then(|p| {
                    p.as_dictionary()
                        .and_then(|p| p.get("Label"))
                        .and_then(plist::Value::as_string)
                        .map(|s| s == self.identity)
                })
                .unwrap_or(false)
        } else {
            std::str::from_utf8(&bytes).is_ok_and(|s| {
                s.lines()
                    .any(|line| line == format!("X-TCM-ITM-Managed={}", self.identity))
            })
        };
        if !owned {
            return Err(failure(
                "An unrecognized registration already occupies this application identity",
            ));
        }
        Ok(Some(bytes))
    }
    pub fn enabled(&self) -> Result<bool> {
        let Some(bytes) = self.existing()? else {
            return Ok(false);
        };
        if self.os == "macos" {
            Ok(
                plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(failure)?
                    == plist::Value::from_reader(std::io::Cursor::new(&self.bytes))
                        .map_err(failure)?,
            )
        } else {
            Ok(bytes == self.bytes)
        }
    }
    pub fn set(&self, enabled: bool) -> Result<()> {
        let before = self.existing()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| failure("Missing login registration parent"))?;
        if !enabled {
            if before.is_some() {
                fs::remove_file(&self.path).map_err(failure)?;
            }
        } else {
            fs::create_dir_all(parent).map_err(failure)?;
            paths::checked(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(failure)?;
            temp.write_all(&self.bytes).map_err(failure)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(failure)?;
            }
            temp.as_file().sync_all().map_err(failure)?;
            if self.existing()? != before {
                return Err(failure("Login registration changed during update"));
            }
            temp.persist(&self.path).map_err(failure)?;
        }
        #[cfg(unix)]
        if parent.exists() {
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(failure)?;
        }
        if self.enabled()? != enabled {
            return Err(failure("Login registration postcondition failed"));
        }
        Ok(())
    }
}
