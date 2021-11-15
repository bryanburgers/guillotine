use super::FutureId;
use libc::c_int;
use std::os::unix::io::AsRawFd;
use std::{io::Error, mem::MaybeUninit};
use tracing::error;

/// A slightly safe structure around `epoll_create`, `epoll_wait`, `epoll_ctl`.
pub struct Epoll {
    /// The file descriptor itself
    fd: c_int,
}

impl Epoll {
    /// Create a new epoll file descriptor
    ///
    /// Roughly equilvanet to `epoll_create1(0)`.
    pub fn new() -> Result<Self, std::io::Error> {
        unsafe {
            let r = libc::epoll_create1(0);
            if r < 0 {
                Err(Error::last_os_error())
            } else {
                Ok(Self { fd: r })
            }
        }
    }

    /// Register a file descriptor with this epoll instance
    ///
    /// Roughly equivalent to `epoll_ctl` with the `EPOLL_CTL_ADD` parameter.
    ///
    /// The provided file descriptor and epoll event are associated with the provided [`FutureId`];
    /// when `wait` is woken it will return the provided `FutureId`.
    pub fn add(&mut self, fd: &impl AsRawFd, future_id: FutureId) -> Result<(), std::io::Error> {
        let fd = fd.as_raw_fd();
        unsafe {
            let events = libc::EPOLLIN | libc::EPOLLOUT | libc::EPOLLET;
            let mut epoll_event = libc::epoll_event {
                events: events as u32,
                u64: future_id.to_u64(),
            };
            let r = libc::epoll_ctl(self.fd, libc::EPOLL_CTL_ADD, fd, &mut epoll_event as *mut _);
            if r < 0 {
                return Err(Error::last_os_error());
            }

            Ok(())
        }
    }

    /// Wait for an event on the epoll instance
    ///
    /// Roughly equivalent to `epoll_wait` with a single event.
    ///
    /// When woken up, the event that triggered the wake up will have a [`FutureId`] associated with
    /// it. This method returns that [`FutureId`] that caused the wake up.
    pub fn wait(&mut self) -> Result<FutureId, std::io::Error> {
        unsafe {
            let mut epoll_event = MaybeUninit::uninit();
            let r = libc::epoll_wait(self.fd, epoll_event.as_mut_ptr(), 1, -1);
            if r < 0 {
                return Err(Error::last_os_error());
            }
            let epoll_event = epoll_event.assume_init();
            let future_id = FutureId::from_u64(epoll_event.u64);

            Ok(future_id)
        }
    }
}

impl Drop for Epoll {
    fn drop(&mut self) {
        unsafe {
            let r = libc::close(self.fd);
            if r < 0 {
                let error = Error::last_os_error();
                error!(error = %error, "failed to close epoll fd");
            }
        }
    }
}
