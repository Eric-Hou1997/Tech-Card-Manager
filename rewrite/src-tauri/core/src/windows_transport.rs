//! Local, process-authenticated Windows maintenance transport. Each operation
//! has a deadline; cancellation is drained before its buffers are released.
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Cryptography::*, *},
    Storage::FileSystem::*,
    System::{Pipes::*, Threading::*, IO::*},
};

pub fn wide(value: impl AsRef<OsStr>) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = value.as_ref().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Embedded NUL"));
    }
    value.push(0);
    Ok(value)
}
/// # Safety
/// `handle` must be a freshly acquired, uniquely owned kernel handle, or a failure sentinel.
pub unsafe fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // The caller passes only newly created, uniquely owned kernel handles.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}
pub struct Descriptor(PSECURITY_DESCRIPTOR);
impl Descriptor {
    pub fn from_sddl(value: &str) -> io::Result<Self> {
        let value = wide(value)?;
        let mut result = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                value.as_ptr(),
                1,
                &mut result,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(result))
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub struct Pipe {
    handle: OwnedHandle,
    deadline: Instant,
}
impl Pipe {
    fn new(handle: OwnedHandle, timeout: Duration) -> Self {
        Self {
            handle,
            deadline: Instant::now() + timeout,
        }
    }
    pub fn reset(&mut self, timeout: Duration) {
        self.deadline = Instant::now() + timeout;
    }
    fn complete(&self, started: i32, operation: &mut OVERLAPPED) -> io::Result<usize> {
        if started == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        let mut transferred = 0;
        let timeout = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u32::MAX as u128 - 1) as u32;
        let wait = unsafe { WaitForSingleObject(operation.hEvent, timeout) };
        if wait != WAIT_OBJECT_0 {
            let wait_error = io::Error::last_os_error();
            // OVERLAPPED and user buffers must live until cancellation completes.
            unsafe {
                CancelIoEx(self.handle.as_raw_handle(), operation);
                GetOverlappedResult(self.handle.as_raw_handle(), operation, &mut transferred, 1);
            }
            return Err(if wait == WAIT_TIMEOUT {
                io::Error::new(io::ErrorKind::TimedOut, "Maintenance pipe deadline expired")
            } else {
                wait_error
            });
        }
        if unsafe {
            GetOverlappedResult(self.handle.as_raw_handle(), operation, &mut transferred, 0)
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(transferred as usize)
        }
    }
    fn event() -> io::Result<OwnedHandle> {
        unsafe { owned(CreateEventW(null(), 1, 0, null())) }
    }
    pub fn client_process(&self) -> io::Result<u32> {
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle.as_raw_handle(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }
    pub fn server_process(&self) -> io::Result<u32> {
        let mut pid = 0;
        if unsafe { GetNamedPipeServerProcessId(self.handle.as_raw_handle(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }
}
impl Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let event = Self::event()?;
        let mut operation = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        };
        let started = unsafe {
            ReadFile(
                self.handle.as_raw_handle(),
                out.as_mut_ptr(),
                out.len().min(u32::MAX as usize) as u32,
                null_mut(),
                &mut operation,
            )
        };
        match self.complete(started, &mut operation) {
            Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            result => result,
        }
    }
}
impl Write for Pipe {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let event = Self::event()?;
        let mut operation = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        };
        let started = unsafe {
            WriteFile(
                self.handle.as_raw_handle(),
                data.as_ptr(),
                data.len().min(u32::MAX as usize) as u32,
                null_mut(),
                &mut operation,
            )
        };
        self.complete(started, &mut operation)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    } // Writes complete above; FlushFileBuffers could wait forever for a reader.
}

pub struct Listener {
    pipe: Pipe,
    name: String,
}
impl Listener {
    pub fn create(timeout: Duration) -> io::Result<Self> {
        let mut random = [0u8; 32];
        if unsafe {
            BCryptGenRandom(
                null_mut(),
                random.as_mut_ptr(),
                random.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } < 0
        {
            return Err(io::Error::other("System random source unavailable"));
        }
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = format!(r"\\.\pipe\tcm-maintenance-{}-{suffix}", std::process::id());
        // Owner rights permit the creator to establish the server. Only elevated
        // administrators/System may connect; no Everyone/Anonymous or remote access.
        let security =
            Descriptor::from_sddl("D:P(A;;GA;;;OW)(A;;0x00120183;;;BA)(A;;0x00120183;;;SY)")?;
        let handle = unsafe {
            owned(CreateNamedPipeW(
                wide(&name)?.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                &security.attributes(),
            ))
        }?;
        Ok(Self {
            pipe: Pipe::new(handle, timeout),
            name,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn accept(self, expected_pid: u32) -> io::Result<Pipe> {
        let event = Pipe::event()?;
        let mut operation = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        };
        let started = unsafe { ConnectNamedPipe(self.pipe.handle.as_raw_handle(), &mut operation) };
        if started == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            self.pipe.complete(started, &mut operation)?;
        }
        if self.pipe.client_process()? != expected_pid || expected_pid <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unexpected maintenance client process",
            ));
        }
        Ok(self.pipe)
    }
}
pub fn connect(name: &str, parent: u32, timeout: Duration) -> io::Result<Pipe> {
    let prefix = format!(r"\\.\pipe\tcm-maintenance-{parent}-");
    let suffix = name
        .strip_prefix(&prefix)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Unexpected pipe namespace"))?;
    if parent <= 1 || suffix.len() != 64 || !suffix.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unexpected pipe identity",
        ));
    }
    // Specific rights exclude FILE_CREATE_PIPE_INSTANCE. Identification prevents
    // an unelevated server from impersonating this administrator client.
    let handle = unsafe {
        owned(CreateFileW(
            wide(name)?.as_ptr(),
            FILE_READ_DATA
                | FILE_WRITE_DATA
                | FILE_READ_ATTRIBUTES
                | FILE_WRITE_ATTRIBUTES
                | READ_CONTROL
                | SYNCHRONIZE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        ))
    }?;
    let pipe = Pipe::new(handle, timeout);
    if pipe.server_process()? != parent {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected maintenance server process",
        ));
    }
    Ok(pipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_pipe_round_trip_and_disconnect() {
        let listener = Listener::create(Duration::from_secs(3)).unwrap();
        let name = listener.name().to_owned();
        let pid = std::process::id();
        let peer = std::thread::spawn(move || {
            let mut pipe = connect(&name, pid, Duration::from_secs(3)).unwrap();
            pipe.write_all(b"hello").unwrap();
            let mut reply = [0; 5];
            pipe.read_exact(&mut reply).unwrap();
            assert_eq!(&reply, b"world");
        });
        let mut pipe = listener.accept(pid).unwrap();
        let mut request = [0; 5];
        pipe.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"hello");
        pipe.write_all(b"world").unwrap();
        peer.join().unwrap();
        assert_eq!(pipe.read(&mut request).unwrap(), 0);
    }
    #[test]
    fn timed_out_read_is_cancelled_before_buffer_reuse() {
        let listener = Listener::create(Duration::from_secs(3)).unwrap();
        let name = listener.name().to_owned();
        let pid = std::process::id();
        let (ready, release) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut pipe = connect(&name, pid, Duration::from_secs(3)).unwrap();
            release.recv().unwrap();
            pipe.write_all(b"later").unwrap();
        });
        let mut pipe = listener.accept(pid).unwrap();
        pipe.reset(Duration::from_millis(20));
        let mut buffer = [0; 5];
        assert_eq!(
            pipe.read(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        pipe.reset(Duration::from_secs(3));
        ready.send(()).unwrap();
        pipe.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"later");
        peer.join().unwrap();
    }
    #[test]
    fn wrong_process_and_external_pipe_names_are_rejected() {
        assert!(connect(
            r"\\remote\pipe\test",
            std::process::id(),
            Duration::from_secs(1)
        )
        .is_err());
        let listener = Listener::create(Duration::from_secs(3)).unwrap();
        let name = listener.name().to_owned();
        let pid = std::process::id();
        let (ready, release) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let pipe = connect(&name, pid, Duration::from_secs(3)).unwrap();
            release.recv().unwrap();
            drop(pipe);
        });
        assert!(
            matches!(listener.accept(pid + 1), Err(e) if e.kind() == io::ErrorKind::PermissionDenied)
        );
        ready.send(()).unwrap();
        peer.join().unwrap();
    }
}
