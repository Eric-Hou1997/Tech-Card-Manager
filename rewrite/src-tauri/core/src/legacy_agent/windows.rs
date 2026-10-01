//! Rename the held, verified file on its original volume. No replacement, copy,
//! ACL reconstruction, shell command or elevation is involved.
use super::*;
use std::{
    fs::OpenOptions,
    io,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Prefix,
};
use windows_sys::Win32::{Foundation::GENERIC_READ, Storage::FileSystem::*};

pub(super) fn identity(file: &File) -> io::Result<Identity> {
    let mut m = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut m) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if m.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || m.nNumberOfLinks != 1
    {
        return Err(io::Error::other("状态文件不是独立普通文件"));
    }
    Ok(Identity {
        file: format!(
            "{:08x}:{:08x}{:08x}",
            m.dwVolumeSerialNumber, m.nFileIndexHigh, m.nFileIndexLow
        ),
        created: format!(
            "{:08x}{:08x}",
            m.ftCreationTime.dwHighDateTime, m.ftCreationTime.dwLowDateTime
        ),
        written: format!(
            "{:08x}{:08x}",
            m.ftLastWriteTime.dwHighDateTime, m.ftLastWriteTime.dwLowDateTime
        ),
        len: ((m.nFileSizeHigh as u64) << 32) | m.nFileSizeLow as u64,
        attributes: m.dwFileAttributes,
    })
}
fn open(path: &Path, rename: bool) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(GENERIC_READ | if rename { DELETE } else { 0 })
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
struct Guard {
    _directories: Vec<File>,
    _engine: File,
}
impl Guard {
    fn new(path: &Path) -> Result<Self> {
        fixed_path(path)?;
        let portable = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| failure(path, "旧目录无效"))?;
        let engine = portable.join("runtime/engine/windows-engine.ps1");
        let mut directories = Vec::new();
        for target in [path, engine.as_path()] {
            let mut walked = PathBuf::new();
            for part in target.parent().expect("absolute file path").components() {
                match part {
                    Component::Prefix(prefix)
                        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
                    {
                        walked.push(part);
                        continue;
                    }
                    Component::Prefix(_) | Component::ParentDir | Component::CurDir => {
                        return Err(failure(target, "旧状态文件须位于明确的本地目录"))
                    }
                    Component::Normal(name) => {
                        let text = name
                            .to_str()
                            .ok_or_else(|| failure(target, "目录编码有歧义"))?;
                        if text.contains(':') || text.ends_with(['.', ' ']) {
                            return Err(failure(target, "目录名称有歧义"));
                        }
                    }
                    _ => {}
                }
                walked.push(part);
                let directory = OpenOptions::new()
                    .access_mode(FILE_READ_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                    .open(&walked)
                    .map_err(|e| failure(&walked, e))?;
                let mut m = BY_HANDLE_FILE_INFORMATION::default();
                if unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &mut m) } == 0 {
                    return Err(failure(&walked, io::Error::last_os_error()));
                }
                if m.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                    || m.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
                {
                    return Err(failure(&walked, "旧目录包含重解析点或不是目录"));
                }
                directories.push(directory);
            }
        }
        let mut file = open(&engine, false).map_err(|e| failure(&engine, e))?;
        let before = identity(&file).map_err(|e| failure(&engine, e))?;
        if before.len != 87186 {
            return Err(failure(&engine, "旧版引擎来源不匹配"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(87187)
            .read_to_end(&mut bytes)
            .map_err(|e| failure(&engine, e))?;
        if hash(&bytes) != crate::legacy_process::BASELINE_ENGINE
            || identity(&file).map_err(|e| failure(&engine, e))? != before
        {
            return Err(failure(&engine, "旧版引擎来源不匹配或已改变"));
        }
        Ok(Self {
            _directories: directories,
            _engine: file,
        })
    }
}
pub(super) fn snapshot(path: &Path) -> Result<Snapshot> {
    let _guard = Guard::new(path)?;
    let path = crate::paths::checked(path)?;
    let mut file = open(&path, false).map_err(|e| failure(&path, e))?;
    let value = read_snapshot(&path, &mut file)?;
    value.validate()?;
    Ok(value)
}
fn observed(path: &Path, expected: &Snapshot, rename: bool) -> Result<Option<File>> {
    let mut file = match open(path, rename) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(failure(path, e)),
    };
    let value = read_snapshot(path, &mut file)?;
    if value.identity != expected.identity || value.contents != expected.contents {
        return Err(failure(path, "旧版状态文件已被替换或改变，未覆盖"));
    }
    Ok(Some(file))
}
fn rename_held(file: &File, destination: &Path) -> Result<()> {
    let name: Vec<_> = destination.as_os_str().encode_wide().collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = (offset + (name.len() + 1) * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    // usize allocation provides pointer alignment; the flexible array follows
    // the SDK's actual offset, including its architecture-specific padding.
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            size as u32,
        ) == 0
        {
            return Err(failure(destination, io::Error::last_os_error()));
        }
    }
    Ok(())
}
/// Exact original object at the deterministic archive is proof of an interrupted
/// rename. Two missing files or any conflicting object remain unverified.
pub fn archive(value: &Snapshot, operation: &str) -> Result<()> {
    move_file(value, operation, false)
}
pub fn restore(value: &Snapshot, operation: &str) -> Result<()> {
    move_file(value, operation, true)
}
fn move_file(value: &Snapshot, operation: &str, restore: bool) -> Result<()> {
    let backup = value.archive_path(operation)?;
    let original = Path::new(&value.path);
    let _guard = Guard::new(original)?;
    let (source, target) = if restore {
        (backup.as_path(), original)
    } else {
        (original, backup.as_path())
    };
    let from = observed(source, value, true)?;
    let to = observed(target, value, false)?;
    match (from, to) {
        (Some(file), None) => {
            rename_held(&file, target)?;
            if identity(&file).map_err(|e| failure(target, e))? != value.identity {
                return Err(failure(target, "状态文件改名后身份改变"));
            }
            // Close DELETE access before reopening for verified postconditions.
            drop(file);
            if observed(source, value, false)?.is_some()
                || observed(target, value, false)?.is_none()
            {
                return Err(failure(target, "状态文件改名后复核未通过"));
            }
            Ok(())
        }
        (None, Some(_)) => Ok(()),
        _ => Err(failure(
            original,
            "状态文件与备份关系不明确，未移动或覆盖文件",
        )),
    }
}
