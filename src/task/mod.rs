//! Spawning tasks separate from the primary future

use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

/// Spawn a new future onto the currently executing runtime
///
/// Panics if there is no runtime currently executing
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + 'static,
    F::Output: 'static,
{
    // Get access to the currently executing runtime, or panic if one isn't running.
    let context = crate::runtime::RuntimeContext::current();

    // When the *spawned* future is completed, the JoinHandle that is returned from this function
    // will need to be polled. To do that, we will need to wake up the future that the JoinHandle is
    // in, which is the *current* future. So get the waker for the current future.
    let waker = context.waker().clone();

    // And with that waker, create the JoinHandle and the "completer", or the thing that will
    // trigger the JoinHandle when the spawned future is done.
    let (handle, completer) = join_handle_pair(waker);

    // Ah, but we're not actually going to spawn the provided future as is. Let's create a new
    // future that waits for the provided future, and then hits the "completer" to tell the
    // JoinHandle the the provided future is done.
    let wrapped_future = async move {
        let result = future.await;
        completer.complete(result)
    };

    // And then add that new wrapped future to the runtime, so it can start executing it when it
    // gets the chance.
    context.spawn(wrapped_future);

    // And finally, hand the JoinHandle back to current future so it can wait for completion if it
    // wants.
    handle
}

/// Spawn a blocking function onto a new thread and provides a join handle to wait for its
/// completion
///
/// Panics if there is no runtime currently executing
pub fn spawn_blocking<Fn, O>(f: Fn) -> JoinHandle<O>
where
    Fn: FnOnce() -> O,
    Fn: Send + 'static,
    O: Send + 'static,
{
    // Get access to the currently executing runtime, or panic if one isn't running.
    let context = crate::runtime::RuntimeContext::current();

    // When the *spawned* future is completed, the JoinHandle that is returned from this function
    // will need to be polled. To do that, we will need to wake up the future that the JoinHandle is
    // in, which is the *current* future. So get the waker for the current future.
    let waker = context.waker().clone();

    // And with that waker, create the JoinHandle and the "completer", or the thing that will
    // trigger the JoinHandle when the spawned future is done.
    let (handle, completer) = join_handle_pair(waker);

    // Ah, but we're not actually going to spawn the provided function as is. Let's create a new
    // function that waits for the provided function, and then hits the "completer" to tell the
    // JoinHandle the the provided function is done.
    let wrapped_function = move || {
        let result = f();
        completer.complete(result)
    };

    // And then spawn that new wrapped function in a new thread.
    std::thread::spawn(wrapped_function);

    // And finally, hand the JoinHandle back to current future so it can wait for completion if it
    // wants.
    handle
}

/// Create a new JoinHandle and a "completer", the thing that will trigger the JoinHandle when the
/// spawned future is done.
pub(crate) fn join_handle_pair<T>(waker: Waker) -> (JoinHandle<T>, JoinHandleCompleter<T>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    (JoinHandle { rx }, JoinHandleCompleter { tx, waker })
}

/// The thing that will trigger the JoinHandle when the future is done.
pub(crate) struct JoinHandleCompleter<T> {
    /// The channel is a place to hold the spawned future's return value until the JoinHandle has a
    /// chance to retrieve it
    tx: std::sync::mpsc::SyncSender<T>,
    /// The waker that can be used to wake up the original future when the JoinHandle is ready.
    waker: Waker,
}

impl<T> JoinHandleCompleter<T> {
    /// Indicate that the spawned future is complete, and the JoinHandle can finish.
    pub fn complete(self, t: T) {
        // Put the spawned future's return value in the channel to indicate that the spawned future
        // has been completed
        match self.tx.send(t) {
            Ok(()) => {
                // And wake up the original future so that the JoinHandle can be polled
                self.waker.wake()
            }
            Err(_) => {
                // The JoinHandle disconnected, which means it was dropped. The original future
                // does not care that the spawned future completed, so it doesn't need to be woken
                // up.
            }
        }
    }
}

/// The handle returned from a [`spawn`]
///
/// This handle can be awaited and will resolve when the spawned future has completed.
#[pin_project]
pub struct JoinHandle<T> {
    /// The other half of the channel.
    ///
    /// If this has something in it, then the spawned future completed and put its result here. If
    /// it doesn't, then the spawned future is still running.
    rx: std::sync::mpsc::Receiver<T>,
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let projected = self.project();
        match projected.rx.try_recv() {
            Err(_) => {
                // Nothing in the channel. The spawned future is still running. Continue to wait.
                Poll::Pending
            }
            Ok(t) => {
                // We got the spawned future's result from the channel. That means the spawned
                // future is done, so so is this JoinHandle.
                Poll::Ready(t)
            }
        }
    }
}
