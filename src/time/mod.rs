//! Time-related futures
//!
//! Sleep for a provided amount of time
//!
//! ```
//! use std::time::Duration;
//!
//! let runtime = guillotine::runtime::Runtime::new().unwrap();
//!
//! let future = async {
//!     guillotine::time::sleep(Duration::from_millis(100)).await.unwrap();
//!     println!("Slept for 100ms");
//! };
//!
//! runtime.block_on(future);
//! ```
//!
//! Or continuously sleep on an interval
//!
//! ```
//! use std::time::Duration;
//!
//! let runtime = guillotine::runtime::Runtime::new().unwrap();
//!
//! let future = async {
//!     let mut interval = guillotine::time::interval(Duration::from_millis(20)).unwrap();
//!     for _ in 0..5 {
//!         interval.tick().await.unwrap();
//!         println!("Slept for 20ms");
//!     }
//!     println!("Slept for 100ms total");
//! };
//!
//! runtime.block_on(future);
//! ```

use crate::runtime::RuntimeContext;
use libc::c_int;
use pin_project::pin_project;
use std::{
    future::Future,
    io::{Error, ErrorKind},
    mem::MaybeUninit,
    os::unix::prelude::AsRawFd,
    time::Duration,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum RegisteredState {
    Unregistered,
    Registered,
}

/// Sleep for the provided amount of time
pub async fn sleep(duration: Duration) -> Result<(), std::io::Error> {
    let sleep = Sleep::new(duration)?;
    sleep.await
}

/// A struct that provides ergonomic access to a `timerfd` file descriptor
struct TimerFd {
    fd: c_int,
}

impl TimerFd {
    /// Create a new `timerfd`
    ///
    /// Roughly equivalent to calling `timerfd_create` and then `timerfd_settime`.
    fn new(interval: Duration, value: Duration) -> Result<Self, std::io::Error> {
        unsafe {
            let fd = libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_NONBLOCK);
            if fd < 0 {
                return Err(Error::last_os_error());
            }

            let spec = libc::itimerspec {
                it_interval: libc::timespec {
                    tv_sec: interval.as_secs() as i64,
                    tv_nsec: interval.subsec_nanos() as i64,
                },
                it_value: libc::timespec {
                    tv_sec: value.as_secs() as i64,
                    tv_nsec: value.subsec_nanos() as i64,
                },
            };
            let mut oldspec: MaybeUninit<libc::itimerspec> = MaybeUninit::uninit();
            let r = libc::timerfd_settime(fd, 0, &spec as *const _, oldspec.as_mut_ptr());
            if r < 0 {
                let err = Error::last_os_error();
                libc::close(fd);
                return Err(err);
            }

            Ok(Self { fd })
        }
    }

    /// Read the value from the file descriptor
    ///
    /// For interval timers, this is typically the number of times the interval has triggered since
    /// the last read.
    fn read(&self) -> Result<u64, std::io::Error> {
        unsafe {
            let mut buf = [0_u8; 8];
            let r = libc::read(self.fd, &mut buf as *mut _ as *mut _, 8);
            if r < 0 {
                Err(Error::last_os_error())
            } else {
                Ok(u64::from_ne_bytes(buf))
            }
        }
    }
}

impl AsRawFd for TimerFd {
    fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
        self.fd
    }
}

impl Drop for TimerFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// The future that runs [`sleep`]
#[pin_project]
struct Sleep {
    /// The timer file descriptor that has been set up for this sleep
    timer: TimerFd,
    /// Whether or not the file descriptor has been registered with epoll
    state: RegisteredState,
}

impl Sleep {
    /// Create a new Sleep
    fn new(duration: Duration) -> Result<Self, std::io::Error> {
        let timer = TimerFd::new(Duration::ZERO, duration)?;
        Ok(Sleep {
            timer,
            state: RegisteredState::Unregistered,
        })
    }
}

impl Future for Sleep {
    type Output = Result<(), std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call read on the file descriptor. Since this is a non-blocking file descriptor, this
        // should return immediately.
        let result = projected.timer.read();
        match result {
            // Success!
            Ok(_) => std::task::Poll::Ready(Ok(())),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(projected.timer);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

/// Create an [`Interval`] that will wait the provided duration before firing, and then will
/// continue to fire on that same duration
pub fn interval(period: Duration) -> Result<Interval, std::io::Error> {
    Interval::new(period)
}

/// An interval that yields a value on a fixed period
pub struct Interval {
    /// The internal timerfd file descriptor that was set up for this interval
    timer: TimerFd,
}

impl Interval {
    /// Create a new interval that will wait the provided duration before firing, and then will
    /// continue to fire on that same duration
    fn new(period: Duration) -> Result<Self, std::io::Error> {
        let timer = TimerFd::new(period, period)?;
        Ok(Interval { timer })
    }

    /// Sleep until the interval fires
    ///
    /// Is the interval would have fired multiple times between calls to this .tick(), the return
    /// value is how many times it would have fired.
    pub async fn tick(&mut self) -> Result<u64, std::io::Error> {
        Tick {
            interval: self,
            state: RegisteredState::Unregistered,
        }
        .await
    }
}

/// The future that runs [`Interval::tick`]
#[pin_project]
struct Tick<'a> {
    interval: &'a mut Interval,
    state: RegisteredState,
}

impl<'a> Future for Tick<'a> {
    type Output = Result<u64, std::io::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let projected = self.project();

        // Call read on the file descriptor. Since this is a non-blocking file descriptor, this
        // should return immediately.
        let result = projected.interval.timer.read();
        match result {
            // Success!
            Ok(ok) => std::task::Poll::Ready(Ok(ok)),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                // Not ready yet. If we haven't registered the file descriptor with the runtime,
                // do it now.
                if *projected.state == RegisteredState::Unregistered {
                    let context = RuntimeContext::current();
                    context.register_file_descriptor(&projected.interval.timer);
                    *projected.state = RegisteredState::Registered;
                }
                std::task::Poll::Pending
            }
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}
