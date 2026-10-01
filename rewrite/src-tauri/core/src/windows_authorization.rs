use super::{
    windows_arguments,
    windows_transport::{owned, wide},
};
use std::{
    ffi::OsString,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::null,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Com::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
};
pub struct Process {
    handle: OwnedHandle,
}
impl Process {
    pub fn authorize(helper: &Path, arguments: &[OsString]) -> io::Result<Self> {
        let helper = helper.to_owned();
        let arguments = arguments.to_owned();
        // Tokio's reusable blocking threads may have another COM apartment.
        // Own and join a fresh STA for this one system authorization operation.
        std::thread::Builder::new()
            .name("tcm-authorization".into())
            .spawn(move || Self::authorize_sta(&helper, &arguments))?
            .join()
            .map_err(|_| io::Error::other("Windows authorization thread failed"))?
    }
    fn authorize_sta(helper: &Path, arguments: &[OsString]) -> io::Result<Self> {
        let initialized = unsafe {
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        if initialized < 0 {
            return Err(io::Error::other(format!(
                "Windows authorization COM initialization failed: {initialized:#x}"
            )));
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let mut parameters = Vec::new();
        for arg in arguments {
            if !parameters.is_empty() {
                parameters.push(b' ' as u16);
            }
            parameters.extend(
                windows_arguments::quote(&arg.encode_wide().collect::<Vec<_>>()).ok_or_else(
                    || {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Embedded NUL in helper arguments",
                        )
                    },
                )?,
            );
        }
        parameters.push(0);
        let file = wide(helper)?;
        let verb = wide("runas")?;
        let mut launch = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: null(),
            nShow: SW_HIDE,
            ..Default::default()
        };
        if unsafe { ShellExecuteExW(&mut launch) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: unsafe { owned(launch.hProcess) }?,
        })
    }
    pub fn id(&self) -> io::Result<u32> {
        let pid = unsafe { GetProcessId(self.handle.as_raw_handle()) };
        if pid <= 1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(pid)
        }
    }
    pub fn wait(&self, timeout: Duration) -> io::Result<Option<u32>> {
        match unsafe {
            WaitForSingleObject(
                self.handle.as_raw_handle(),
                timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
}
