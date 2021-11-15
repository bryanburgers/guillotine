use libc::c_int;
use std::{io::Error, os::unix::prelude::AsRawFd};
use tracing::error;

/// A structure that represents a linux `eventfd` file descriptor.
pub struct EventFd {
    /// The file descriptor itself
    fd: c_int,
}

impl EventFd {
    /// Create a new file descriptor.
    ///
    /// Roughly equivalent to calling `eventfd`.
    pub fn new() -> Result<Self, std::io::Error> {
        unsafe {
            let r = libc::eventfd(0, libc::EFD_NONBLOCK);
            if r < 0 {
                Err(Error::last_os_error())
            } else {
                Ok(Self { fd: r })
            }
        }
    }

    /// Read from the file descriptor.
    ///
    /// Oh, I guess we never actually do this. Well, what do you expect; this isn't a
    /// production-grade futures executor.
    pub fn _read(&self) -> Result<u64, std::io::Error> {
        unsafe {
            let mut bytes = [0_u8; 8];
            let r = libc::read(self.fd, &mut bytes as *mut u8 as *mut libc::c_void, 8);
            if r < 0 {
                Err(Error::last_os_error())
            } else {
                Ok(u64::from_ne_bytes(bytes))
            }
        }
    }

    /// Write to the file descriptor.
    ///
    /// For an `eventfd` file descriptor, this pretty much just triggers the event. And in our case
    /// it causes a wakeup on `epoll`.
    pub fn write(&self, val: u64) -> Result<(), std::io::Error> {
        unsafe {
            let data = val.to_ne_bytes();
            let r = libc::write(self.fd, &data as *const u8 as *const libc::c_void, 8);
            if r < 0 {
                Err(Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
        self.fd
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        unsafe {
            let r = libc::close(self.fd);
            if r < 0 {
                let error = Error::last_os_error();
                error!(error = %error, "failed to close eventfd fd");
            }
        }
    }
}
