//! Fail-closed ACL checks for elevated maintenance. Keep ancestor handles open
//! without delete sharing throughout the session so a checked directory cannot
//! be exchanged between a check and a journal/integration operation.
use super::windows_transport::{owned, wide, Descriptor};
use std::{
    ffi::OsString,
    io,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Com::CoTaskMemFree, Threading::*},
    UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
};
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
struct LocalMemory(*mut std::ffi::c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
unsafe fn sid_string(sid: PSID) -> io::Result<String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(denied("Invalid Windows owner/ACL SID"));
    }
    let mut text = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _memory = LocalMemory(text.cast());
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    Ok(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(text, length)
    }))
}
pub struct Policy {
    trusted_installer: Option<String>,
}
impl Policy {
    pub fn new() -> io::Result<Self> {
        let account = wide("NT SERVICE\\TrustedInstaller")?;
        let (mut sid_len, mut domain_len, mut kind) = (0, 0, 0);
        unsafe {
            LookupAccountNameW(
                null(),
                account.as_ptr(),
                null_mut(),
                &mut sid_len,
                null_mut(),
                &mut domain_len,
                &mut kind,
            );
        }
        let mut sid = vec![0u32; (sid_len as usize).div_ceil(4)];
        let mut domain = vec![0u16; domain_len as usize];
        let trusted_installer = if sid_len > 0
            && unsafe {
                LookupAccountNameW(
                    null(),
                    account.as_ptr(),
                    sid.as_mut_ptr().cast(),
                    &mut sid_len,
                    domain.as_mut_ptr(),
                    &mut domain_len,
                    &mut kind,
                )
            } != 0
        {
            Some(unsafe { sid_string(sid.as_mut_ptr().cast()) }?)
        } else {
            None
        };
        Ok(Self { trusted_installer })
    }
    fn trusted(&self, sid: &str) -> bool {
        sid == "S-1-5-18" || sid == "S-1-5-32-544" || self.trusted_installer.as_deref() == Some(sid)
    }
    fn acl(&self, handle: &OwnedHandle, ancestor: bool, private: bool) -> io::Result<()> {
        let (mut owner, mut acl, mut sd) = (null_mut(), null_mut(), null_mut());
        let code = unsafe {
            GetSecurityInfo(
                handle.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut sd,
            )
        };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let _memory = LocalMemory(sd);
        if !self.trusted(&unsafe { sid_string(owner) }?)
            || acl.is_null()
            || unsafe { IsValidAcl(acl) } == 0
        {
            return Err(denied(
                "Maintenance requires an administrator-controlled owner and explicit DACL",
            ));
        }
        // Creating an unrelated child in ProgramData/volume root does not permit
        // replacing our existing child. Existing ancestors themselves must remain
        // protected against delete, child delete, ownership and DACL changes.
        let changes = if ancestor {
            DELETE
                | WRITE_DAC
                | WRITE_OWNER
                | FILE_DELETE_CHILD
                | FILE_WRITE_EA
                | FILE_WRITE_ATTRIBUTES
                | GENERIC_ALL
                | GENERIC_WRITE
        } else {
            DELETE
                | WRITE_DAC
                | WRITE_OWNER
                | FILE_DELETE_CHILD
                | FILE_WRITE_DATA
                | FILE_APPEND_DATA
                | FILE_WRITE_EA
                | FILE_WRITE_ATTRIBUTES
                | GENERIC_ALL
                | GENERIC_WRITE
        };
        for index in 0..unsafe { (*acl).AceCount } as u32 {
            let mut raw = null_mut();
            if unsafe { GetAce(acl, index, &mut raw) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*raw.cast::<ACE_HEADER>() };
            if header.AceFlags as u32 & INHERIT_ONLY_ACE != 0 {
                continue;
            }
            match header.AceType {
                0 => {
                    if (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                        return Err(denied("Invalid access-control entry"));
                    }
                    let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
                    let sid =
                        unsafe { sid_string(std::ptr::addr_of!(ace.SidStart).cast_mut().cast()) }?;
                    if !self.trusted(&sid) && (private || ace.Mask & changes != 0) {
                        return Err(denied("An unprivileged identity can modify the maintenance target or read private recovery data"));
                    }
                }
                1 => {} // Ignoring deny entries makes this policy more restrictive.
                _ => {
                    return Err(denied(
                        "Unsupported conditional/object ACL requires administrator review",
                    ))
                }
            }
        }
        Ok(())
    }
    pub fn object(
        &self,
        path: &Path,
        directory: bool,
        ancestor: bool,
        private: bool,
    ) -> io::Result<OwnedHandle> {
        let handle = unsafe {
            owned(CreateFileW(
                wide(path)?.as_ptr(),
                FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            ))
        }?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
            || (!directory && info.nNumberOfLinks != 1)
        {
            return Err(denied("Reparse points, ambiguous object kinds and hard-linked files are not maintenance targets"));
        }
        self.acl(&handle, ancestor, private)?;
        Ok(handle)
    }
    pub fn tree(&self, path: &Path, private: bool) -> io::Result<Vec<OwnedHandle>> {
        let mut parts = path.components();
        if !matches!(parts.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || !matches!(parts.next(), Some(Component::RootDir))
        {
            return Err(denied("Maintenance requires an absolute local drive path"));
        }
        for part in parts {
            match part {
                Component::Normal(name) => {
                    let name = name.to_string_lossy();
                    if name.contains(':') || name.ends_with([' ', '.']) {
                        return Err(denied("Ambiguous Windows path rejected"));
                    }
                }
                _ => return Err(denied("Ambiguous Windows path rejected")),
            }
        }
        let mut held = Vec::new();
        let mut ancestors: Vec<_> = path.ancestors().filter(|p| p.has_root()).collect();
        ancestors.reverse();
        for part in ancestors {
            held.push(self.object(part, true, part != path, private && part == path)?);
        }
        Ok(held)
    }
    pub fn journal(&self) -> io::Result<(PathBuf, Vec<OwnedHandle>)> {
        let mut text = null_mut();
        let status =
            unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, null_mut(), &mut text) };
        if status < 0 {
            return Err(io::Error::other(format!(
                "ProgramData lookup failed: {status:#x}"
            )));
        }
        let mut length = 0;
        while unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        let parent = PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(text, length)
        }));
        unsafe {
            CoTaskMemFree(text.cast());
        }
        // Pin and validate ancestors before creating the private directory.
        let mut held = Vec::new();
        let mut ancestors: Vec<_> = parent.ancestors().filter(|p| p.has_root()).collect();
        ancestors.reverse();
        for part in ancestors {
            held.push(self.object(part, true, true, false)?);
        }
        let root = parent.join(if crate::OFFICIAL_RELEASE {
            "TechCardManager"
        } else {
            "TechCardManagerValidation"
        });
        let security = Descriptor::from_sddl("O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")?;
        if unsafe { CreateDirectoryW(wide(&root)?.as_ptr(), &security.attributes()) } == 0
            && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
        {
            return Err(io::Error::last_os_error());
        }
        held.push(self.object(&root, true, false, true)?);
        self.journal_contents(&root)?;
        Ok((root, held))
    }
    pub fn journal_contents(&self, root: &Path) -> io::Result<()> {
        let mut todo = vec![root.to_owned()];
        while let Some(directory) = todo.pop() {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let directory = entry.file_type()?.is_dir();
                self.object(&entry.path(), directory, false, true)?;
                if directory {
                    todo.push(entry.path());
                }
            }
        }
        Ok(())
    }
}
pub fn elevated() -> io::Result<()> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { owned(token) }?;
    let mut value = TOKEN_ELEVATION::default();
    let mut length = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenElevation,
            (&mut value as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if value.TokenIsElevated == 0 {
        return Err(denied(
            "Launch maintenance through Windows administrator authorization",
        ));
    }
    Ok(())
}
/// Called only in the dedicated, authenticated elevated helper, before it opens
/// its Session. New recovery artifacts must belong to Administrators rather than
/// the interactive user, whose unelevated token could otherwise change the DACL.
pub fn prepare_helper_owner() -> io::Result<()> {
    elevated()?;
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { owned(token) }?;
    let mut sid = null_mut();
    if unsafe { ConvertStringSidToSidW(wide("S-1-5-32-544")?.as_ptr(), &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _memory = LocalMemory(sid);
    let owner = TOKEN_OWNER { Owner: sid };
    if unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenOwner,
            (&owner as *const TOKEN_OWNER).cast(),
            std::mem::size_of::<TOKEN_OWNER>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn parent_process(pid: u32) -> io::Result<OwnedHandle> {
    let process = unsafe {
        owned(OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        ))
    }?;
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return Err(denied("Manager is no longer running"));
    }
    let mut value = vec![0u16; 32768];
    let mut length = value.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, value.as_mut_ptr(), &mut length)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let actual = PathBuf::from(OsString::from_wide(&value[..length as usize])).canonicalize()?;
    let expected = std::env::current_exe()?
        .parent()
        .ok_or_else(|| denied("Helper directory missing"))?
        .join(format!("{}.exe", crate::MANAGER_BINARY_NAME))
        .canonicalize()?;
    if actual != expected {
        return Err(denied("Helper must be launched by its packaged Manager"));
    }
    Ok(process)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_traversal_and_alias_paths_are_rejected_before_opening() {
        let policy = Policy::new().unwrap();
        for path in [
            r"\\server\share\web",
            r"C:\web\..\Windows",
            r"C:\web:stream",
            "C:\\web. ",
        ] {
            assert!(policy.tree(Path::new(path), false).is_err(), "{path}");
        }
    }
    #[test]
    fn multiple_link_files_fail_before_acl_acceptance() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        std::fs::write(&original, b"fixture").unwrap();
        std::fs::hard_link(&original, temp.path().join("alias")).unwrap();
        let error = Policy::new()
            .unwrap()
            .object(&original, false, false, false)
            .unwrap_err();
        assert!(error.to_string().contains("hard-linked"));
        assert_eq!(std::fs::read(original).unwrap(), b"fixture");
    }
    #[test]
    #[ignore = "Requires an explicitly elevated Windows test process; never invokes UAC from a unit test"]
    fn protected_acl_accepts_admin_only_and_rejects_public_write_or_readable_recovery() {
        elevated().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let policy = Policy::new().unwrap();
        for (name, sddl, public_ok, private_ok) in [
            (
                "private",
                "O:BAG:BAD:P(A;OICI;FA;;;BA)(A;OICI;FA;;;SY)",
                true,
                true,
            ),
            (
                "readable",
                "O:BAG:BAD:P(A;OICI;FA;;;BA)(A;OICI;FR;;;WD)",
                true,
                false,
            ),
            (
                "writable",
                "O:BAG:BAD:P(A;OICI;FA;;;BA)(A;OICI;FW;;;WD)",
                false,
                false,
            ),
        ] {
            let path = temp.path().join(name);
            let descriptor = Descriptor::from_sddl(sddl).unwrap();
            assert_ne!(
                unsafe {
                    CreateDirectoryW(wide(&path).unwrap().as_ptr(), &descriptor.attributes())
                },
                0
            );
            assert_eq!(policy.object(&path, true, false, false).is_ok(), public_ok);
            assert_eq!(policy.object(&path, true, false, true).is_ok(), private_ok);
        }
    }
}
