//! Read-only evidence for the original Windows Manager. A filename is only a
//! candidate filter; these observations never authorize process termination.
use crate::{hash, paths, AppError, Result};
use serde::Serialize;
use std::{fs::File, io::Read, path::Path};

pub const BASELINE_ENGINE: &str =
    "30b55ab5cd107fef8747ffbd202e290c8738d0e644de6864e9b3adb0795a61f5";
const ENGINE_PREFIX: &[u8] = b"\xef\xbb\xbfparam(\n    [switch]$IndexOnly,";
const HTML_PREFIX: &[u8] = b"<!doctype html>\n<!-- Tech Card Manager v4.1.0";

pub fn candidate_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "tech-card-manager.exe" | "tech card manager.exe"
    ) || (name.starts_with("imdbtechmanager") && name.ends_with(".exe"))
}

fn contains_asset(bytes: &[u8], prefix: &[u8], size: usize, digest: &str) -> bool {
    bytes
        .windows(prefix.len())
        .enumerate()
        .any(|(offset, value)| {
            value == prefix
                && bytes
                    .get(offset..offset + size)
                    .is_some_and(|value| hash(value) == digest)
        })
}

/// Go embeds both original assets unchanged. Matching their complete digests is
/// provenance evidence, not a code signature or proof of the loaded image bytes.
pub fn baseline_assets_match(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ")
        && contains_asset(bytes, ENGINE_PREFIX, 87186, BASELINE_ENGINE)
        && contains_asset(
            bytes,
            HTML_PREFIX,
            98455,
            "d11b8e873b7c801fd057ac09fa808eb32cfa737eb73bf63855c01db8b1bc7078",
        )
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ExecutableEvidence {
    pub path: String,
    pub sha256: String,
    pub baseline_assets_match: bool,
}
pub fn executable_evidence(path: &Path) -> Result<ExecutableEvidence> {
    path_text(path)?;
    let path = paths::checked(path)?;
    let text = path_text(&path)?.to_owned();
    let bytes = read_file(&path, 128 << 20)?;
    Ok(ExecutableEvidence {
        path: text,
        sha256: hash(&bytes),
        baseline_assets_match: baseline_assets_match(&bytes),
    })
}
fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| {
        AppError::new("legacy-process-path", "程序路径编码有歧义，无法确认归属").at(path.display())
    })
}
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let path = paths::checked(path)?;
    let mut file = File::open(&path)
        .map_err(|e| AppError::new("legacy-process-read", e).at(path.display()))?;
    let before = file
        .metadata()
        .map_err(|e| AppError::new("legacy-process-read", e).at(path.display()))?;
    if !before.is_file() || before.len() > limit {
        return Err(AppError::new("legacy-process-size", "无法核对旧版程序文件").at(path.display()));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::new("legacy-process-read", e).at(path.display()))?;
    let after = file
        .metadata()
        .map_err(|e| AppError::new("legacy-process-read", e).at(path.display()))?;
    if bytes.len() as u64 != before.len()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err(
            AppError::new("legacy-process-changed", "旧版程序文件在核对期间改变")
                .at(path.display()),
        );
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentFile {
    pub path: String,
    pub sha256: String,
    pub identity: String,
    pub pid: Option<u32>,
    pub error: Option<AppError>,
}

/// Fixed state files in a baseline-verified portable directory. A PID is
/// historical metadata only; neither existence nor syntax proves a live owner.
pub fn agent_files(portable: &Path) -> Result<Vec<AgentFile>> {
    path_text(portable)?;
    let portable = paths::checked(portable)?;
    path_text(&portable)?;
    let engine = portable.join("runtime/engine/windows-engine.ps1");
    if hash(&read_file(&engine, 8 << 20)?) != BASELINE_ENGINE {
        return Err(
            AppError::new("legacy-agent-source", "无法确认旧版 Agent 状态文件来源")
                .at(portable.display()),
        );
    }
    let mut files = Vec::new();
    for name in ["agent.pid", "agent-heartbeat.txt"] {
        let path = portable.join("data").join(name);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(AppError::new("legacy-agent-read", error).at(path.display())),
            Ok(_) => {}
        }
        let snapshot = crate::legacy_agent::snapshot(&path)?;
        let bytes = &snapshot.contents;
        let mut file = AgentFile {
            path: path_text(&path)?.to_owned(),
            sha256: hash(bytes),
            identity: snapshot.fingerprint()?,
            pid: None,
            error: None,
        };
        if name == "agent.pid" {
            file.pid = std::str::from_utf8(bytes)
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .filter(|pid| *pid > 0);
            if file.pid.is_none() {
                file.error = Some(
                    AppError::new(
                        "legacy-agent-pid",
                        "旧版 PID 文件无效，不能据此确认运行状态",
                    )
                    .at(path.display()),
                );
            }
        }
        files.push(file);
    }
    Ok(files)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessObservation {
    pub pid: u32,
    pub name: String,
    pub created: Option<String>,
    pub executable: Option<ExecutableEvidence>,
    pub error: Option<AppError>,
}

pub fn verify_same_process(
    reviewed: &ProcessObservation,
    current: &ProcessObservation,
) -> Result<()> {
    let verified = reviewed.error.is_none()
        && current.error.is_none()
        && reviewed.pid != 0
        && reviewed.pid == current.pid
        && reviewed
            .created
            .as_ref()
            .is_some_and(|time| time.len() == 16 && time.bytes().all(|c| c.is_ascii_hexdigit()))
        && reviewed.created == current.created
        && reviewed
            .executable
            .as_ref()
            .zip(current.executable.as_ref())
            .is_some_and(|(before, now)| {
                before.baseline_assets_match
                    && now.baseline_assets_match
                    && before.path == now.path
                    && before.sha256.len() == 64
                    && before.sha256 == now.sha256
            });
    if !verified {
        return Err(AppError::new(
            "legacy-process-review-changed",
            "旧版程序状态或归属已改变，请重新确认迁移清单",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn read_identity(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> Result<(String, ExecutableEvidence)> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let fail = |code| AppError::new(code, std::io::Error::last_os_error());
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(fail("legacy-process-identity"));
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return Err(fail("legacy-process-path"));
    }
    path.truncate(length as usize);
    let executable = executable_evidence(Path::new(&OsString::from_wide(&path)))?;
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_TIMEOUT => Ok((
            format!(
                "{:08x}{:08x}",
                created.dwHighDateTime, created.dwLowDateTime
            ),
            executable,
        )),
        WAIT_OBJECT_0 => Err(AppError::new(
            "legacy-process-exited",
            "旧版候选程序已退出，需要重新检查",
        )),
        _ => Err(fail("legacy-process-state")),
    }
}

/// Invoke only after the product's itemized confirmation and an unchanged
/// inventory. This closes the exact original tray host; it never taskkills a
/// name/PID tree or requests process termination rights.
#[cfg(windows)]
pub fn request_stop(reviewed: &ProcessObservation) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
    verify_same_process(reviewed, reviewed)?;
    if reviewed.pid == unsafe { GetCurrentProcessId() } {
        return Err(AppError::new("legacy-process-self", "不能迁移当前程序"));
    }
    let fail = |code| AppError::new(code, std::io::Error::last_os_error());
    let handle = unsafe {
        crate::windows_transport::owned(OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            reviewed.pid,
        ))
    }
    .map_err(|e| AppError::new("legacy-process-access", e))?;
    let current = || -> Result<ProcessObservation> {
        let (created, executable) = read_identity(handle.as_raw_handle())?;
        Ok(ProcessObservation {
            pid: unsafe { GetProcessId(handle.as_raw_handle()) },
            name: reviewed.name.clone(),
            created: Some(created),
            executable: Some(executable),
            error: None,
        })
    };
    verify_same_process(reviewed, &current()?)?;
    struct Windows {
        pid: u32,
        handles: Vec<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, context: LPARAM) -> windows_sys::core::BOOL {
        let state = unsafe { &mut *(context as *mut Windows) };
        let mut pid = 0;
        if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } != 0 && pid == state.pid {
            let mut class = [0u16; 128];
            let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
            if len > 0
                && class[..len as usize]
                    .iter()
                    .copied()
                    .eq("TechCardManagerTrayHost400".encode_utf16())
            {
                state.handles.push(hwnd);
            }
        }
        1
    }
    let mut windows = Windows {
        pid: reviewed.pid,
        handles: Vec::new(),
    };
    if unsafe { EnumWindows(Some(visit), &mut windows as *mut Windows as LPARAM) } == 0 {
        return Err(fail("legacy-process-windows"));
    }
    if windows.handles.len() != 1 {
        return Err(AppError::new(
            "legacy-process-tray",
            "无法唯一确认旧版程序的退出入口",
        ));
    }
    let hwnd = windows.handles[0];
    // Re-read image and instance after window discovery; the held handle keeps
    // this process instance distinct from any later reuse of its PID.
    verify_same_process(reviewed, &current()?)?;
    let mut pid = 0;
    let mut class = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0
        || pid != reviewed.pid
        || len <= 0
        || !class[..len as usize]
            .iter()
            .copied()
            .eq("TechCardManagerTrayHost400".encode_utf16())
    {
        return Err(AppError::new(
            "legacy-process-review-changed",
            "旧版程序退出入口已改变",
        ));
    }
    let mut result = 0;
    // This API can fail without setting last-error. Do not report an unrelated
    // earlier error (or ERROR_SUCCESS) as the cause of a rejected close.
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    if unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_CLOSE,
            0,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            2000,
            &mut result,
        )
    } == 0
    {
        let error = unsafe { GetLastError() };
        if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        return Err(AppError::new(
            "legacy-process-close",
            if error == ERROR_SUCCESS {
                "旧版程序未确认关闭请求".to_owned()
            } else {
                std::io::Error::from_raw_os_error(error as i32).to_string()
            },
        ));
    }
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 10000) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(AppError::new(
            "legacy-process-stop-timeout",
            "旧版程序尚未确认退出，迁移未继续",
        )),
        _ => Err(fail("legacy-process-state")),
    }
}

#[cfg(windows)]
pub fn observe() -> Result<Vec<ProcessObservation>> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    let failure = |code| AppError::new(code, std::io::Error::last_os_error());
    let snapshot =
        unsafe { crate::windows_transport::owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)) }
            .map_err(|e| AppError::new("legacy-process-enumeration", e))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut present = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    let mut results = Vec::new();
    loop {
        if present == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(failure("legacy-process-enumeration"));
            }
            break;
        }
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        let pid = entry.th32ProcessID;
        if pid != unsafe { GetCurrentProcessId() } && candidate_name(&name) {
            let mut result = ProcessObservation {
                pid,
                name,
                created: None,
                executable: None,
                error: None,
            };
            let observed = (|| -> Result<()> {
                // Hold one process handle while reading identity. No taskkill,
                // shell, PID-file inference, or termination rights are used.
                let handle = unsafe {
                    crate::windows_transport::owned(OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                        0,
                        pid,
                    ))
                }
                .map_err(|e| AppError::new("legacy-process-access", e))?;
                let (created, executable) = read_identity(handle.as_raw_handle())?;
                result.created = Some(created);
                result.executable = Some(executable);
                Ok(())
            })();
            result.error = observed.err();
            results.push(result);
        }
        present = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    results.sort_by_key(|process| process.pid);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn process_evidence_rejects_paths_that_would_collapse_under_lossy_encoding() {
        use std::os::unix::ffi::OsStringExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for byte in [0xfe, 0xff] {
            let path = root.join(std::ffi::OsString::from_vec(vec![b'a', byte]));
            assert_eq!(
                executable_evidence(&path).unwrap_err().code,
                "legacy-process-path"
            );
            assert_eq!(agent_files(&path).unwrap_err().code, "legacy-process-path");
        }
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
    }
    #[test]
    fn process_stop_review_rejects_reused_pids_changed_images_and_unknown_ownership() {
        let original = ProcessObservation {
            pid: 123,
            name: "Tech-Card-Manager.exe".into(),
            created: Some("0123456789abcdef".into()),
            executable: Some(ExecutableEvidence {
                path: "C:\\Original\\Tech-Card-Manager.exe".into(),
                sha256: "a".repeat(64),
                baseline_assets_match: true,
            }),
            error: None,
        };
        verify_same_process(&original, &original).unwrap();
        let mut changed = original.clone();
        changed.name = "not-an-ownership-claim.exe".into();
        verify_same_process(&original, &changed).unwrap();
        for field in [
            "pid",
            "creation",
            "path",
            "hash",
            "assets",
            "error",
            "missing-image",
            "missing-time",
        ] {
            let mut changed = original.clone();
            match field {
                "pid" => changed.pid += 1,
                "creation" => changed.created = Some("1123456789abcdef".into()),
                "path" => {
                    changed.executable.as_mut().unwrap().path =
                        "C:\\Other\\Tech-Card-Manager.exe".into()
                }
                "hash" => changed.executable.as_mut().unwrap().sha256 = "b".repeat(64),
                "assets" => changed.executable.as_mut().unwrap().baseline_assets_match = false,
                "error" => changed.error = Some(AppError::new("read-denied", "unknown")),
                "missing-image" => changed.executable = None,
                "missing-time" => changed.created = None,
                _ => unreachable!(),
            }
            assert_eq!(
                verify_same_process(&original, &changed).unwrap_err().code,
                "legacy-process-review-changed",
                "{field}"
            );
        }
        let mut invalid = original.clone();
        invalid.created = Some("not-an-instance".into());
        assert!(verify_same_process(&invalid, &invalid).is_err());
        invalid = original.clone();
        invalid.pid = 0;
        assert!(verify_same_process(&invalid, &invalid).is_err());
    }
    #[test]
    fn agent_files_preserve_bytes_and_pids_never_claim_a_running_process() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("runtime/engine")).unwrap();
        std::fs::create_dir(root.join("data")).unwrap();
        let engine = root.join("runtime/engine/windows-engine.ps1");
        std::fs::write(
            &engine,
            include_bytes!("../../../../windows/engine/windows-engine.ps1"),
        )
        .unwrap();
        assert!(agent_files(&root).unwrap().is_empty());
        let pid = root.join("data/agent.pid");
        let heartbeat = root.join("data/agent-heartbeat.txt");
        std::fs::write(&pid, b" 456\r\n").unwrap();
        std::fs::write(&heartbeat, b"stale timestamp\r\n").unwrap();
        let modified = std::fs::metadata(&pid).unwrap().modified().unwrap();
        let files = agent_files(&root).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].pid, Some(456));
        assert!(files[0].error.is_none());
        assert_eq!(std::fs::read(&pid).unwrap(), b" 456\r\n");
        assert_eq!(
            std::fs::metadata(&pid).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(std::fs::read(&heartbeat).unwrap(), b"stale timestamp\r\n");
        for invalid in ["0", "-1", "456 extra", "4294967296"] {
            std::fs::write(&pid, invalid).unwrap();
            let observed = agent_files(&root).unwrap();
            assert!(observed[0].pid.is_none());
            assert!(observed[0].error.is_some());
        }
        std::fs::write(&engine, b"unknown engine").unwrap();
        assert_eq!(agent_files(&root).unwrap_err().code, "legacy-agent-source");
    }
    #[cfg(unix)]
    #[test]
    fn agent_symlinks_are_not_treated_as_absent_or_owned_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("runtime/engine")).unwrap();
        std::fs::create_dir(root.join("data")).unwrap();
        std::fs::write(
            root.join("runtime/engine/windows-engine.ps1"),
            include_bytes!("../../../../windows/engine/windows-engine.ps1"),
        )
        .unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("data/agent.pid")).unwrap();
        assert_eq!(agent_files(&root).unwrap_err().code, "ambiguous-path");
        assert!(!root.join("missing").exists());
    }
    #[test]
    #[ignore = "Requires an explicitly compiled original Windows executable via TCM_BASELINE_EXE"]
    fn compiled_original_program_contains_the_expected_baseline_assets() {
        let path = std::env::var_os("TCM_BASELINE_EXE").expect("TCM_BASELINE_EXE is required");
        let evidence = executable_evidence(Path::new(&path)).unwrap();
        assert!(evidence.baseline_assets_match, "{}", evidence.path);
    }
    #[test]
    fn original_embedded_assets_are_checked_in_full_without_executing_the_program() {
        let engine = include_bytes!("../../../../windows/engine/windows-engine.ps1");
        let html = include_bytes!("../../../../windows/web/index.html");
        assert_eq!(hash(engine), BASELINE_ENGINE);
        let mut image = b"MZfixture only, never executable".to_vec();
        image.extend_from_slice(engine);
        image.extend_from_slice(html);
        let temp = tempfile::tempdir().unwrap();
        let path = temp
            .path()
            .canonicalize()
            .unwrap()
            .join("Tech-Card-Manager.exe");
        std::fs::write(&path, &image).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let evidence = executable_evidence(&path).unwrap();
        assert!(evidence.baseline_assets_match);
        assert_eq!(evidence.sha256, hash(&image));
        assert_eq!(std::fs::read(&path).unwrap(), image);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        image[128] ^= 1;
        assert!(!baseline_assets_match(&image));
        assert!(!baseline_assets_match(b"MZ Tech Card Manager v4.1.0"));
        let mut single = b"MZ".to_vec();
        single.extend_from_slice(engine);
        assert!(!baseline_assets_match(&single));
        assert!(!baseline_assets_match(&image[..100]));
    }
    #[test]
    fn process_names_only_filter_candidates_and_ambiguous_paths_fail_closed() {
        for name in [
            "Tech-Card-Manager.exe",
            "Tech Card Manager.EXE",
            "IMDbTechManager.exe",
            "IMDbTechManager-old.exe",
        ] {
            assert!(candidate_name(name));
        }
        for name in [
            "ITM.exe",
            "EmbyServer.exe",
            "Tech-Card-Manager.exe.txt",
            "other-IMDbTechManager.exe",
        ] {
            assert!(!candidate_name(name));
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert!(executable_evidence(&root).is_err());
        assert!(executable_evidence(&root.join("missing.exe")).is_err());
        #[cfg(unix)]
        {
            let exe = root.join("other.exe");
            std::fs::write(&exe, b"MZunrelated").unwrap();
            let link = root.join("Tech-Card-Manager.exe");
            std::os::unix::fs::symlink(&exe, &link).unwrap();
            assert_eq!(
                executable_evidence(&link).unwrap_err().code,
                "ambiguous-path"
            );
            assert!(!executable_evidence(&exe).unwrap().baseline_assets_match);
        }
    }
}
