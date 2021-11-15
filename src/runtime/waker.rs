//! Everything to get a `Waker` set up to be provided to `Future::poll`.
//!
//! Each future executor provides its own custom implementation of a `Waker` to futures by creating
//! a `RawWaker` from a pointer and a VTable to specific functions.
//!
//! This is pretty much like a trait implementation, but externalizes some of the trait metadata, so
//! we have to handle it ourselves.
//!
//! For our implementation, the *pointer* is going to be an `Arc<GuillotineWaker>`. Any time one of
//! the VTable functions is called, we turn the pointer into an `Arc<GuillotineWaker>`, forward the
//! call to the correct function on `GuillotineWaker`, and then either drop the Arc or don't drop
//! the Arc, depending on what the VTable function expects.

use super::eventfd;
use std::sync::Arc;
use std::task::{RawWaker, RawWakerVTable, Waker};

/// The waker that is responsible for waking up the runtime when a future is ready to be polled
///
/// Internally, this uses an `eventfd` file descriptor, and when the waker needs to wake up the
/// associated future, it writes to that file descriptor, which wakes up epoll, which causes the
/// executor to poll the future associated with this file descriptor.
struct GuillotineWaker {
    eventfd: eventfd::EventFd,
}

impl GuillotineWaker {
    /// Create a new waker
    pub fn new(eventfd: eventfd::EventFd) -> Self {
        GuillotineWaker { eventfd }
    }

    /// Wake up the runtime!
    pub fn wake(&self) {
        // Write to the file descriptor to wake up epoll
        self.eventfd
            .write(1)
            .expect("Ahh! What do we do if this fails?");
    }
}

/// The `Future` trait requires a `Context`, which requires a `Waker`, which requires a `RawWaker`,
/// which requires a `RawWakerVTable`
///
/// The VTable is, I guess, just a pointer to a bunch of free functions that can be called.
const GUILLOTINE_VTABLE: RawWakerVTable = RawWakerVTable::new(
    guillotine_clone,
    guillotine_wake,
    guillotine_wake_by_ref,
    guillotine_drop,
);

/// The free function that is called when `Waker.clone()` is called
///
/// This function gives us a chance to clone or internal waker in whatever way we need to. We don't
/// need to do anything special, so just clone the Arc to create a new RawWaker.
unsafe fn guillotine_clone(data: *const ()) -> RawWaker {
    // Turn the pointer into an Arc.
    let original = Arc::from_raw(data as *const GuillotineWaker);
    // Because this is the `clone` VTable entry, clone the Arc so we can make a new RawMaker.
    let cloned = original.clone();
    // The contract for this function is that the caller retains access to the original pointer, so
    // we don't want to drop the orginal Arc
    std::mem::forget(original);
    // And return a new (cloned) RawWaker.
    RawWaker::new(Arc::into_raw(cloned) as *const (), &GUILLOTINE_VTABLE)
}

/// The free function that is called when `Waker.wake()` is called
///
/// Get access to our internal waker and call `.wake()` on it.
unsafe fn guillotine_wake(data: *const ()) {
    // Turn the pointer into an Arc.
    let arc = Arc::from_raw(data as *const GuillotineWaker);
    // Because this is the `wake` VTable entry, perform the wakeup.
    arc.wake();
    // `Waker.wake()` *consumes* the waker, so we need to drop the Arc after we've called wake.
    std::mem::drop(arc);
}

/// The free function that is called when `Waker.wake_by_ref()` is called
///
/// Get access to our internal waker and call `.wake()` on it.
unsafe fn guillotine_wake_by_ref(data: *const ()) {
    // Turn the pointer into an Arc.
    let arc = Arc::from_raw(data as *const GuillotineWaker);
    // Because this is the `wake_by_ref` VTable entry, perform the wakeup. Note that for our
    // implementation, there is no difference between a `wake` and a `wake_by_ref`; they perform
    // the same wakeup.
    arc.wake();
    // However, because this is `wake_by_ref`, the caller retains ownership of the Waker, so we do
    // not want to drop the Arc here.
    std::mem::forget(arc);
}

/// The free function that is called when a Waker goes out of scope
///
/// This function gives us chance to do some special drop code. We don't need to do anything
/// special, so just drop the Arc
unsafe fn guillotine_drop(data: *const ()) {
    // Turn the pointer into an Arc.
    let arc = Arc::from_raw(data as *const GuillotineWaker);
    // Because this is the `drop` VTable entry, drop the Arc.
    std::mem::drop(arc)
}

/// Build a new waker from the eventfd.
pub fn build(eventfd: eventfd::EventFd) -> Waker {
    // Create a new internal waker
    let guillotine_waker = Arc::new(GuillotineWaker::new(eventfd));
    // Turn it into a pointer, because that's what RawWaker wants
    let pointer = Arc::into_raw(guillotine_waker) as *const ();
    // The pointer and the VTable make a RawWaker
    let raw_waker = RawWaker::new(pointer, &GUILLOTINE_VTABLE);
    // This is unsafe because it's on us to guarantee that we're respecting the contract throughout
    // all of these unsafe VTable calls. We are.
    let waker = unsafe { Waker::from_raw(raw_waker) };
    waker
}
