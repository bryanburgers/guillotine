//! The bit that actually runs the futures

mod context;
mod epoll;
mod eventfd;
mod future_id;
mod waker;

pub(crate) use context::RuntimeContext;
use future_id::{FutureId, FutureIdGenerator};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::rc::Rc;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};
use tracing::warn;

/// The parts of the runtime that need to be exposed to internal futures
pub(crate) struct RuntimeInner {
    /// The epoll instance that drives the entire runtime
    ///
    /// This needs to be exposed because we allow internal futures to register their file
    /// descriptors with this instance.
    epoll: epoll::Epoll,
    /// The next future ID to hand out
    ///
    /// This needs to be exposed for when we spawn a new future, we need to give that future a
    /// unique identifier
    future_id_generator: FutureIdGenerator,
    /// All of the new futures that have been spawned
    ///
    /// This needs to be exposed because when we span a new future, we need a place to put it
    new_futures: VecDeque<(FutureId, Pin<Box<dyn Future<Output = ()>>>)>,
}

impl RuntimeInner {
    /// Create a new instance of this.
    fn new() -> Result<Self, std::io::Error> {
        let epoll = epoll::Epoll::new()?;
        let future_id_generator = FutureIdGenerator::default();
        let new_futures = VecDeque::new();

        Ok(Self {
            epoll,
            future_id_generator,
            new_futures,
        })
    }

    /// Spawn a new future into the runtime by adding it to the `new_futures` list.
    pub fn spawn<F>(&mut self, future: F) -> FutureId
    where
        F: Future<Output = ()> + 'static,
    {
        // Get a unique future identifier
        let future_id = self.future_id_generator.fresh();

        // Pin the future. This does the type erasure right here, and we need it to be pinned anyway
        // so here is as good of a place as any.
        let future = Box::pin(future);

        // Throw it into the list of new futures! Next time the executor gets around to executing,
        // it will pull futures off out of this list.
        self.new_futures.push_back((future_id, future));

        future_id
    }
}

/// The bit that actually runs the futures
pub struct Runtime {
    /// A good chunk is in `RuntimeInner`, so we can spawn futures and such into it
    inner: Rc<RefCell<RuntimeInner>>,
    /// The list of all of the futures we know about, though, doesn't need to be shared around.
    /// we'll hord that all to ourselves.
    ///
    /// When we register a file descriptor with epoll, we register what [`FutureId`] it's for. So
    /// when we get an event from epoll, we need a way to look up the relevant future by its ID.
    futures: HashMap<FutureId, (Waker, Pin<Box<dyn Future<Output = ()>>>)>,
}

impl Runtime {
    /// Create a new runtime
    ///
    /// Because this creates the epoll, it could fail.
    pub fn new() -> Result<Self, std::io::Error> {
        let inner = Rc::new(RefCell::new(RuntimeInner::new()?));
        let futures = HashMap::new();

        Ok(Self { inner, futures })
    }

    /// Block the runtime until the future completes, returning the result of the future
    ///
    /// This is the primary entry point to the runtime.
    ///
    /// Technically, this blocks until *all* futures are complete. And the returns the results of
    /// the future given.
    ///
    /// ```
    /// let runtime = guillotine::runtime::Runtime::new().unwrap();
    /// let r = runtime.block_on(async { 42 });
    /// assert_eq!(r, 42);
    /// ```
    pub fn block_on<F>(self, future: F) -> F::Output
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        // The channel is just a place to store the result once the future finishes with it.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        // Create a new future that runs the provided future, and sticks the result in the channel.
        let wrapped_future = async move {
            let result = future.await;
            tx.send(result).unwrap();
        };

        // Put the future into the runtime and then run the runtime until it's done.
        self.spawn(wrapped_future);
        self.block();

        // Because all of the futures are done, we know our wrapped future is done. So we can now
        // grab the result out of the channel and away we go!
        rx.recv().expect("Expected to recv")
    }

    /// Block until all of the futures have executed to completion
    ///
    /// You probably want to use [`Runtime::block_on`], but [`Runtime::block_on`] uses this method
    /// internally.
    ///
    /// This method doesn't take a future: to use this, [`Runtime::spawn`] futures onto the runtime
    /// before calling this method.
    ///
    /// ```
    /// let runtime = guillotine::runtime::Runtime::new().unwrap();
    ///
    /// // Spawn three separate futures
    /// runtime.spawn(async {});
    /// runtime.spawn(async {});
    /// runtime.spawn(async {});
    ///
    /// // Block until all of them have completed
    /// runtime.block();
    /// ```
    pub fn block(mut self) {
        let _block_guard = tracing::info_span!("block").entered();

        // Run until we've exhaused every future
        loop {
            // Check if there are any *new* futures that have been spawned that we need to deal
            // with. If there are, take the first one.
            let front = {
                let mut inner = self.inner.try_borrow_mut().expect("Expected mutex to lock");
                inner.new_futures.pop_front()
            };

            // If there weren't any new futures *AND* there aren't any existing futures, then, uh,
            // there are no futures. We're done.
            if front.is_none() && self.futures.is_empty() {
                // Later, gator.
                break;
            }

            if let Some((future_id, mut new_future)) = front {
                // If there was a new future that needs to be dealt with
                let _new_future_guard =
                    tracing::info_span!("future", future_id = %future_id, status = "new").entered();

                // Create a new waker. `Future::poll` requires that we have a waker so that a future
                // can be woken up later when it's ready. Our waker wraps an eventfd file descriptor
                // that we've put into epoll. When the waker gets called, it writes to that eventfd
                // which wakes the epoll, and things can continue.
                let waker = self.create_waker(future_id);
                let mut context = Context::from_waker(&waker);

                // Our internal futures need a way to access this Runtime. There's nothing in the
                // Future trait that lets that happen, so we set a thread local variable with some
                // context that our futures can use while they're being polled, and then we clear
                // it afterward.
                //
                // So set it here...
                RuntimeContext::set(RuntimeContext::new(
                    future_id,
                    waker.clone(),
                    self.inner.clone(),
                ));

                // ...poll the future...
                let result = {
                    let _poll_guard = tracing::info_span!("poll").entered();
                    new_future.as_mut().poll(&mut context)
                };

                // ...and clear the context.
                RuntimeContext::clear();

                // What should we do with the result of the poll?
                match result {
                    Poll::Ready(()) => {
                        // It ran to completion already!? That was quick. Then we don't even need
                        // to save it. Let it go out of scope. See ya!
                    }
                    Poll::Pending => {
                        // It didn't finish. So we need to store it away in our list of long-term
                        // futures that we continue to poll until comppletion.
                        self.futures.insert(future_id, (waker, new_future));
                    }
                }
            } else {
                // There are no new futures that need to be dealt with.

                // So let's wait until one of our current futures needs to be dealt with. epoll will
                // block until a file descriptor says it's ready. This could be a TCP or UDP file
                // descriptor. Or it could be one of our eventfd file descriptors that exists to
                // wake us up when a future should be polled again. Either way, wait until
                // *something* wakes us up again.
                //
                // When epoll does wake up, it will tell us which future it woke up for.
                let future_id = {
                    let mut inner = self.inner.try_borrow_mut().expect("Expected mutex to lock");
                    inner
                        .epoll
                        .wait()
                        .expect("What do we do if epoll_wait fails?")
                };

                let _future_guard =
                    tracing::info_span!("future", future_id = %future_id, status = "existing")
                        .entered();

                // Lifetimes. There's maybe a way to do this better, but let's use a bool to
                // determine if the future we're going to execute is finished or not.
                let mut should_remove = false;

                // Get the future that woke us up.
                if let Some((waker, future)) = self.futures.get_mut(&future_id) {
                    let mut context = Context::from_waker(&waker);

                    // Our internal futures need a way to access this Runtime. There's nothing in
                    // the Future trait that lets that happen, so we set a thread local variable
                    // with some context that our futures can use while they're being polled, and
                    // then we clear it afterward.
                    //
                    // So set it here...
                    RuntimeContext::set(RuntimeContext::new(
                        future_id,
                        waker.clone(),
                        self.inner.clone(),
                    ));

                    // ...poll the future...
                    let result = {
                        let _poll_guard = tracing::info_span!("poll").entered();
                        future.as_mut().poll(&mut context)
                    };

                    // ...and clear the context.
                    RuntimeContext::clear();
                    match result {
                        Poll::Ready(()) => {
                            // The future is done. We no longer need to deal with it.
                            should_remove = true;
                        }
                        Poll::Pending => {
                            // The future did not complete. So we leave it in our stash of running
                            // futures until the next time it's ready to be polled.
                        }
                    }
                } else {
                    warn!(future_id = ?future_id, "epoll returned future_id that was not expected");
                }

                // If we should remove it, then, uh, remove it.
                if should_remove {
                    self.futures.remove(&future_id);
                }
            }
        }
    }

    /// Create a waker for a particular future
    fn create_waker(&mut self, future_id: FutureId) -> Waker {
        let fd = eventfd::EventFd::new().expect("What do we do when this panics!?");

        {
            self.inner
                .try_borrow_mut()
                .expect("Expected mutex to lock")
                .epoll
                .add(&fd, future_id)
                .expect("What do we do if epoll add fails?");
        }

        waker::build(fd)
    }

    /// Spawn a future onto the runtime before running
    ///
    /// Typically, you'll want to use [`Runtime::block_on`] and run a single future to completion.
    /// But if for some reason you want to spawn a handful of futures onto the executor to all be
    /// run at the same time, well here you go.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let mut inner = self.inner.try_borrow_mut().expect("Expected mutex to lock");
        inner.spawn(future);
    }
}
