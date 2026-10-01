//! Current-user login registration and the separate, read-only 4.1.0 locator.
//! Old data still requires the importer's engine digest and settings checks.
use crate::{windows_arguments, AppError, Result};
use std::{
    ffi::OsString,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Registry::*};

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

#[derive(serde::Serialize)]
pub struct LegacyRunEntry {
    pub name: String,
    pub present: Option<bool>,
    pub current_program: bool,
    pub command_sha256: Option<String>,
    pub executable: Option<String>,
    pub source: Option<crate::legacy_process::ExecutableEvidence>,
    pub mode: Option<String>,
    pub error: Option<AppError>,
}

/// Read the two original fixed values. Unknown commands are retained as a
/// digest, never evaluated, expanded, printed, or treated as absent.
pub fn legacy_entries() -> Result<Vec<LegacyRunEntry>> {
    let current = std::env::current_exe().map_err(|e| AppError::new("legacy-startup-path", e))?;
    let mut entries = Vec::new();
    for name in ["Tech Card Manager", "IMDb Tech Manager Agent"] {
        let reader = LoginRegistration::new(name, &current)?;
        let mut entry = LegacyRunEntry {
            name: name.into(),
            present: None,
            current_program: false,
            command_sha256: None,
            executable: None,
            source: None,
            mode: None,
            error: None,
        };
        let result = (|| -> Result<()> {
            let Some(key) = reader.open(None, false)? else {
                entry.present = Some(false);
                return Ok(());
            };
            let Some(command) = reader.read(&key)? else {
                entry.present = Some(false);
                return Ok(());
            };
            entry.present = Some(true);
            entry.current_program =
                windows_arguments::owns_login_command(&command, &reader.executable);
            entry.command_sha256 = Some(crate::hash(
                &command
                    .iter()
                    .flat_map(|unit| unit.to_le_bytes())
                    .collect::<Vec<_>>(),
            ));
            let parsed = windows_arguments::original_login_executable(&command)
                .map(|path| (path, "login"))
                .or_else(|| {
                    windows_arguments::original_agent_executable(&command)
                        .map(|path| (path, "agent"))
                });
            if let Some((path, mode)) = parsed {
                let text = String::from_utf16(&path)
                    .map_err(|e| reader.error("legacy-startup-path", e))?;
                let path = PathBuf::from(&text);
                if path.is_absolute() {
                    entry.executable = Some(text);
                    entry.mode = Some(mode.into());
                    if !entry.current_program {
                        entry.source = Some(crate::legacy_process::executable_evidence(&path)?);
                    }
                }
            }
            Ok(())
        })();
        entry.error = result.err();
        entries.push(entry);
    }
    Ok(entries)
}

pub struct LoginRegistration {
    name: Vec<u16>,
    executable: Vec<u16>,
    command: Vec<u16>,
    path: String,
}

pub fn prepare_legacy_run(name: &str) -> Result<Option<crate::legacy_startup::RunBackup>> {
    use crate::{legacy_process::executable_evidence, legacy_startup::RunBackup};
    if !matches!(name, "Tech Card Manager" | "IMDb Tech Manager Agent") {
        return Err(AppError::new("legacy-startup-name", "未知的旧登录项名称"));
    }
    let current = std::env::current_exe().map_err(|e| AppError::new("legacy-startup-path", e))?;
    let reader = LoginRegistration::new(name, &current)?;
    let Some(key) = reader.open(None, false)? else {
        return Ok(None);
    };
    let Some(command) = reader.read(&key)? else {
        return Ok(None);
    };
    if windows_arguments::owns_login_command(&command, &reader.executable) {
        return Err(reader.error(
            "legacy-startup-current",
            "当前程序的登录项不属于旧组件清理范围",
        ));
    }
    let path = match name {
        "Tech Card Manager" => windows_arguments::original_login_executable(&command),
        _ => windows_arguments::original_agent_executable(&command),
    }
    .ok_or_else(|| {
        reader.error(
            "legacy-startup-unowned",
            "旧登录项内容无法确认，已保留原记录",
        )
    })?;
    let path = String::from_utf16(&path).map_err(|e| reader.error("legacy-startup-path", e))?;
    let backup = RunBackup {
        name: name.into(),
        command,
        executable: executable_evidence(Path::new(&path))?,
    };
    backup.path()?;
    Ok(Some(backup))
}

/// Only call after the product has confirmed the complete migration plan.
/// The backup must already be durably stored by the unelevated user's Store.
/// HKCU here is deliberately the app user's hive, never an alternate UAC user.
pub fn remove_legacy_run(
    store: &crate::store::Store,
    id: &str,
    fingerprint: &str,
) -> Result<crate::legacy_startup::Removal> {
    use crate::legacy_startup::{Removal, RunPhase};
    let receipt = store.begin_legacy_run_removal(id, fingerprint)?;
    let result = remove_legacy_run_value(store, fingerprint)?;
    store.advance_legacy_run(
        &receipt,
        match result {
            Removal::Removed => RunPhase::Removed,
            Removal::AlreadyAbsent => RunPhase::Absent,
        },
    )?;
    Ok(result)
}

fn remove_legacy_run_value(
    store: &crate::store::Store,
    fingerprint: &str,
) -> Result<crate::legacy_startup::Removal> {
    use crate::{legacy_process::executable_evidence, legacy_startup::Removal};
    let backup = store.legacy_run_backup(fingerprint)?;
    let path = backup.path()?;
    let current = std::env::current_exe().map_err(|e| AppError::new("legacy-startup-path", e))?;
    let reader = LoginRegistration::new(&backup.name, &current)?;
    let transaction = reader.transaction()?;
    let Some(key) = reader.open(Some(transaction.0.as_raw_handle()), false)? else {
        return Ok(Removal::AlreadyAbsent);
    };
    let existing = reader.read(&key)?;
    if !backup.matches_command(existing.as_deref())? {
        return Ok(Removal::AlreadyAbsent);
    };
    if windows_arguments::owns_login_command(&backup.command, &reader.executable) {
        return Err(reader.error(
            "legacy-startup-current",
            "当前程序的登录项不属于旧组件清理范围",
        ));
    }
    let actual = executable_evidence(Path::new(&path))?;
    if !actual.baseline_assets_match
        || actual.path != backup.executable.path
        || actual.sha256 != backup.executable.sha256
    {
        return Err(reader.error(
            "legacy-startup-source-changed",
            "旧登录项程序来源已改变，未删除登录项",
        ));
    }
    reader.checked(unsafe { RegDeleteValueW(key.0, reader.name.as_ptr()) })?;
    if reader.read(&key)?.is_some() {
        return Err(reader.error("legacy-startup-verification", "旧登录项删除后复核失败"));
    }
    if unsafe { CommitTransaction(transaction.0.as_raw_handle()) } == 0 {
        return Err(reader.error(
            "legacy-startup-transaction",
            std::io::Error::last_os_error(),
        ));
    }
    if let Some(observed) = reader.open(None, false)? {
        if reader.read(&observed)?.is_some() {
            return Err(reader.error(
                "legacy-startup-verification",
                "旧登录项状态再次改变，请重新检查",
            ));
        }
    }
    Ok(Removal::Removed)
}

/// Compensate a confirmed removal in the same user's hive. A backup or an
/// interrupted removal alone never authorizes restoring an autostart command.
/// Does not launch the old program, and never overwrites another current value.
pub fn restore_legacy_run(store: &crate::store::Store, id: &str) -> Result<()> {
    use crate::{legacy_process::executable_evidence, legacy_startup::RunPhase};
    let receipt = store.legacy_run_receipt(id)?;
    let receipt = match receipt.phase {
        RunPhase::Removed => store.advance_legacy_run(&receipt, RunPhase::Restoring)?,
        RunPhase::Restoring | RunPhase::Restored => receipt,
        _ => {
            return Err(AppError::new(
                "legacy-startup-restore-unverified",
                "无法确认旧登录项由本次操作删除，未恢复登录项",
            ))
        }
    };
    let backup = store.legacy_run_backup(&receipt.backup)?;
    let path = backup.path()?;
    let current = std::env::current_exe().map_err(|e| AppError::new("legacy-startup-path", e))?;
    let reader = LoginRegistration::new(&backup.name, &current)?;
    if windows_arguments::owns_login_command(&backup.command, &reader.executable) {
        return Err(reader.error(
            "legacy-startup-current",
            "当前程序的登录项不属于旧组件恢复范围",
        ));
    }
    let transaction = reader.transaction()?;
    let key = reader
        .open(
            Some(transaction.0.as_raw_handle()),
            receipt.phase != RunPhase::Restored,
        )?
        .ok_or_else(|| reader.error("legacy-startup-restore", "无法打开登录项恢复目标"))?;
    let existing = reader.read(&key)?;
    // Exact original is the only replayable value after a lost restore reply.
    let already_present = backup.matches_command(existing.as_deref())?;
    if receipt.phase == RunPhase::Restored && !already_present {
        return Err(reader.error(
            "legacy-startup-verification",
            "已恢复的旧登录项状态再次改变，未重新写入",
        ));
    }
    let actual = executable_evidence(Path::new(&path))?;
    if !actual.baseline_assets_match
        || actual.path != backup.executable.path
        || actual.sha256 != backup.executable.sha256
    {
        return Err(reader.error(
            "legacy-startup-source-changed",
            "旧登录项程序来源已改变，未恢复登录项",
        ));
    }
    if !already_present {
        let bytes: Vec<_> = backup
            .command
            .iter()
            .copied()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        reader.checked(unsafe {
            RegSetValueExW(
                key.0,
                reader.name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        })?;
    }
    if reader.read(&key)?.as_deref() != Some(backup.command.as_slice()) {
        return Err(reader.error("legacy-startup-verification", "旧登录项恢复后复核失败"));
    }
    if unsafe { CommitTransaction(transaction.0.as_raw_handle()) } == 0 {
        return Err(reader.error(
            "legacy-startup-transaction",
            std::io::Error::last_os_error(),
        ));
    }
    let observed = reader
        .open(None, false)?
        .ok_or_else(|| reader.error("legacy-startup-verification", "旧登录项恢复后状态再次改变"))?;
    if reader.read(&observed)?.as_deref() != Some(backup.command.as_slice()) {
        return Err(reader.error("legacy-startup-verification", "旧登录项恢复后状态再次改变"));
    }
    if receipt.phase == RunPhase::Restored {
        return Ok(());
    }
    match store.advance_legacy_run(&receipt, RunPhase::Restored) {
        Ok(_) => Ok(()),
        Err(error) => {
            // Another retry may have verified and committed the same receipt.
            let latest = store.legacy_run_receipt(id)?;
            if latest.backup == receipt.backup && latest.phase == RunPhase::Restored {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}
impl LoginRegistration {
    pub fn new(name: &str, executable: &Path) -> Result<Self> {
        if name.is_empty() || name.contains(['\0', '\\', '/']) || !executable.is_absolute() {
            return Err(AppError::new("autostart-path", "无法确定当前程序路径"));
        }
        let path = format!("HKCU\\{RUN}\\{name}");
        let name =
            crate::windows_transport::wide(name).map_err(|e| AppError::new("autostart-path", e))?;
        let executable: Vec<_> = executable.as_os_str().encode_wide().collect();
        let command = windows_arguments::login_command(&executable)
            .ok_or_else(|| AppError::new("autostart-path", "无法确定当前程序路径"))?;
        Ok(Self {
            name,
            executable,
            command,
            path,
        })
    }
    fn error(&self, code: &str, error: impl ToString) -> AppError {
        AppError::new(code, error).at(&self.path)
    }
    fn checked(&self, status: WIN32_ERROR) -> Result<()> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(self.error(
                "autostart-registry",
                std::io::Error::from_raw_os_error(status as i32),
            ))
        }
    }
    fn open(&self, transaction: Option<HANDLE>, create: bool) -> Result<Option<Key>> {
        let key: Vec<_> = RUN.encode_utf16().chain([0]).collect();
        let mut handle = null_mut();
        let status = unsafe {
            match transaction {
                Some(transaction) if create => RegCreateKeyTransactedW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    null(),
                    &mut handle,
                    null_mut(),
                    transaction,
                    null(),
                ),
                Some(transaction) => RegOpenKeyTransactedW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    0,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    &mut handle,
                    transaction,
                    null(),
                ),
                None => RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    0,
                    KEY_QUERY_VALUE,
                    &mut handle,
                ),
            }
        };
        if matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
            return Ok(None);
        }
        self.checked(status)?;
        Ok(Some(Key(handle)))
    }
    fn read(&self, key: &Key) -> Result<Option<Vec<u16>>> {
        let mut bytes = vec![0u8; 65536];
        let mut size = bytes.len() as u32;
        let mut kind = 0;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                self.name.as_ptr(),
                null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        self.checked(status)?;
        if kind != REG_SZ || size as usize > bytes.len() {
            return Err(self.error("autostart-conflict", "登录启动项格式无法确认，已保留原记录"));
        }
        bytes.truncate(size as usize);
        windows_arguments::registry_string(&bytes)
            .map(Some)
            .ok_or_else(|| self.error("autostart-conflict", "登录启动项格式无法确认，已保留原记录"))
    }
    pub fn enabled(&self) -> Result<bool> {
        let Some(key) = self.open(None, false)? else {
            return Ok(false);
        };
        Ok(self
            .read(&key)?
            .is_some_and(|value| windows_arguments::owns_login_command(&value, &self.executable)))
    }
    fn transaction(&self) -> Result<Transaction> {
        Ok(Transaction(
            unsafe {
                crate::windows_transport::owned(CreateTransaction(
                    null_mut(),
                    null_mut(),
                    0,
                    0,
                    0,
                    5000,
                    null(),
                ))
            }
            .map_err(|e| self.error("autostart-transaction", e))?,
        ))
    }
    pub fn set(&self, enabled: bool) -> Result<()> {
        // TxR binds the ownership check and change. A concurrent registry writer
        // must fail the transaction rather than have an unrelated value deleted.
        let transaction = self.transaction()?;
        let Some(key) = self.open(Some(transaction.0.as_raw_handle()), enabled)? else {
            return Ok(());
        };
        let existing = self.read(&key)?;
        if existing
            .as_ref()
            .is_some_and(|value| !windows_arguments::owns_login_command(value, &self.executable))
        {
            return Err(self.error("autostart-conflict", "登录启动项指向其他程序，已保留原记录"));
        }
        if enabled {
            let bytes: Vec<_> = self
                .command
                .iter()
                .copied()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            self.checked(unsafe {
                RegSetValueExW(
                    key.0,
                    self.name.as_ptr(),
                    0,
                    REG_SZ,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            })?;
        } else if existing.is_some() {
            self.checked(unsafe { RegDeleteValueW(key.0, self.name.as_ptr()) })?;
        }
        let expected = enabled.then(|| self.command.clone());
        if self.read(&key)? != expected {
            return Err(self.error("autostart-verification", "Windows 登录自启动写入后校验失败"));
        }
        if unsafe { CommitTransaction(transaction.0.as_raw_handle()) } == 0 {
            return Err(self.error("autostart-transaction", std::io::Error::last_os_error()));
        }
        // Lifecycle also verifies the external state before persisting settings.
        if self.enabled()? != enabled {
            return Err(self.error("autostart-verification", "Windows 登录自启动写入后校验失败"));
        }
        Ok(())
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
struct Transaction(OwnedHandle);
impl Drop for Transaction {
    fn drop(&mut self) {
        unsafe {
            RollbackTransaction(self.0.as_raw_handle());
        }
    }
}

pub fn legacy_portable() -> Result<Option<PathBuf>> {
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "Tech Card Manager\0".encode_utf16().collect();
    let mut buffer = vec![0u16; 32768];
    let mut length = (buffer.len() * 2) as u32;
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_NOEXPAND,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if matches!(
        code,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_UNSUPPORTED_TYPE
    ) {
        return Ok(None);
    }
    if code != ERROR_SUCCESS {
        return Err(AppError::new(
            "migration-startup-read",
            std::io::Error::from_raw_os_error(code as i32),
        )
        .at("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\Tech Card Manager"));
    }
    if length < 2 || !length.is_multiple_of(2) || length as usize > buffer.len() * 2 {
        return Err(AppError::new(
            "migration-startup-invalid",
            "旧版登录启动项格式无效",
        ));
    }
    buffer.truncate(length as usize / 2);
    if buffer.pop() != Some(0) {
        return Err(AppError::new(
            "migration-startup-invalid",
            "旧版登录启动项格式无效",
        ));
    }
    let Some(executable) = windows_arguments::original_login_executable(&buffer) else {
        return Ok(None);
    };
    let executable = PathBuf::from(OsString::from_wide(&executable));
    if !executable.is_absolute() {
        return Ok(None);
    }
    Ok(executable.parent().map(std::path::Path::to_path_buf))
}
