use super::{FutureId, RuntimeInner};
use std::{
    cell::RefCell, future::Future, io::ErrorKind, os::unix::prelude::AsRawFd, rc::Rc, task::Waker,
};

/// The current context of the executing runtime.
///
/// The [`Future`] trait does not have any way to get the current runtime from the future being
/// polled. It does have a [`Waker`] that is *provided* by the current runtime, but the Waker also
/// has no way to access the current runtime other than to wake it.
///
/// So this structure provides a way to get the current runtime, by setting the context as a
/// thread-local variable that is set right before a future is polled, and cleared immediately
/// afterward.
#[derive(Clone)]
pub(crate) struct RuntimeContext {
    /// The part of the runtime that is exposed to the context
    inner: Rc<RefCell<RuntimeInner>>,
    /// The waker that is associated with the current task
    waker: Waker,
    /// The future ID that is associated with the current task
    future_id: FutureId,
}

thread_local! {
    /// The thread-local variable that represents the current context
    ///
    /// This is private; external uses (even within the crate) need to use `RuntimeContext::set` and
    /// `RuntimeContext::clear`.
    static RUNTIME_CONTEXT: RefCell<Option<RuntimeContext>> = RefCell::new(None);
}

impl RuntimeContext {
    /// Create a new context
    pub fn new(future_id: FutureId, waker: Waker, inner: Rc<RefCell<RuntimeInner>>) -> Self {
        Self {
            inner,
            waker,
            future_id,
        }
    }

    /// Get the current context from the thread-local variable
    ///
    /// If there is no current context, this will panic.
    pub fn current() -> RuntimeContext {
        if let Some(context) = Self::try_current() {
            context
        } else {
            panic!("No active runtime")
        }
    }

    /// Get the current context from the thread-local variable
    ///
    /// Like `current()`, but uses an `Option` instead of panicking.
    pub fn try_current() -> Option<RuntimeContext> {
        if let Some(context) = RUNTIME_CONTEXT.with(|cell| cell.borrow().clone()) {
            Some(context)
        } else {
            None
        }
    }

    /// Set the provided runtime as the current runtime.
    pub fn set(context: RuntimeContext) {
        RUNTIME_CONTEXT.with(|runtime_context| runtime_context.replace(Some(context)));
    }

    /// Clear the current context so there is no current.
    pub fn clear() {
        RUNTIME_CONTEXT.with(|runtime_context| runtime_context.replace(None));
    }

    /// Get a reference to the currently executing future's waker.
    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    /// Spawn a new futures onto the currently executing runtime.
    pub fn spawn<F>(&self, future: F) -> FutureId
    where
        F: Future<Output = ()> + 'static,
    {
        let mut inner = self.inner.try_borrow_mut().expect("Expected to lock inner");
        inner.spawn(future)
    }

    /// Register a file descriptor with the currently executing runtime's epoll instance
    ///
    /// The provided file descriptor will be associated with the currently executing future's ID, so
    /// any time the file descriptor wakes up epoll because it is ready, the current future will be
    /// polled.
    pub fn register_file_descriptor(&self, fd: &impl AsRawFd) {
        let mut inner = self.inner.try_borrow_mut().expect("Expected to lock inner");
        match inner.epoll.add(fd, self.future_id) {
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                // Listen, this isn't a production-grade runtime. We're definitely not using epoll
                // in the best way. Part of that is that our internal futures will often try to add
                // the same file descriptor to the epoll multiple times.
                //
                // Instead of fixing this problem, we're just going to ignore it. The point of this
                // library was to learn about future executors, not epoll.
                //
                // If this makes you mad, go look at Mio or something.
            }
            r => r.expect("Expected to add successfully"),
        }
    }
}
