//! Private inherited descriptors with a deadline for the entire request/response.
//! No addresses or credential-bearing handshake files are exposed.
use std::{
    io::{self, Read, Write},
    os::fd::{AsRawFd, OwnedFd},
    time::{Duration, Instant},
};
pub struct Pipe {
    fd: OwnedFd,
    deadline: Instant,
}
impl Pipe {
    pub fn new(fd: OwnedFd, timeout: Duration) -> io::Result<Self> {
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd,
            deadline: Instant::now() + timeout,
        })
    }
    pub fn reset(&mut self, timeout: Duration) {
        self.deadline = Instant::now() + timeout;
    }
    fn ready(&self, events: i16) -> io::Result<()> {
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Maintenance pipe deadline expired",
                ));
            }
            let mut poll = libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events,
                revents: 0,
            };
            let timeout = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
            let result = unsafe { libc::poll(&mut poll, 1, timeout) };
            if result > 0 {
                if poll.revents & libc::POLLNVAL != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "Maintenance descriptor closed",
                    ));
                }
                return Ok(()); // Read observes EOF on HUP; write observes EPIPE.
            }
            if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err(io::Error::last_os_error());
            }
        }
    }
}
impl Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            self.ready(libc::POLLIN)?;
            let count =
                unsafe { libc::read(self.fd.as_raw_fd(), out.as_mut_ptr().cast(), out.len()) };
            if count >= 0 {
                return Ok(count as usize);
            }
            let error = io::Error::last_os_error();
            if !matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                return Err(error);
            }
        }
    }
}
impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            self.ready(libc::POLLOUT)?;
            let count =
                unsafe { libc::write(self.fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
            if count >= 0 {
                return Ok(count as usize);
            }
            let error = io::Error::last_os_error();
            if !matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                return Err(error);
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    #[test]
    fn deadline_eof_and_partial_frame_do_not_block_cleanup() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut reader = Pipe::new(a.into(), Duration::from_millis(30)).unwrap();
        let mut writer = Pipe::new(b.into(), Duration::from_secs(1)).unwrap();
        writer.write_all(&[0, 0]).unwrap();
        assert_eq!(
            reader.read_exact(&mut [0; 4]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        reader.reset(Duration::from_secs(1));
        drop(writer);
        assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    }
    #[test]
    fn blocked_writer_expires_instead_of_holding_the_parent() {
        let (a, _b) = UnixStream::pair().unwrap();
        let mut writer = Pipe::new(a.into(), Duration::from_millis(30)).unwrap();
        assert_eq!(
            writer
                .write_all(&vec![1; 4 * 1024 * 1024])
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
