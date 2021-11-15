use pin_project::pin_project;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Waker,
    thread,
    time::Duration,
};
use tracing::{debug, info};
use tracing_subscriber::prelude::*;

fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let runtime = guillotine::runtime::Runtime::new().unwrap();

    let future = async {
        info!("In future!");

        let waker_test = WakerTestFuture::new(Duration::from_secs(2));
        waker_test.await;

        7
    };
    let result = runtime.block_on(future);
    info!(result = result);
}

#[pin_project]
#[derive(Debug)]
struct WakerTestFuture {
    state: WakerTestFutureState,
    ready: Arc<AtomicBool>,
}

impl WakerTestFuture {
    fn new(duration: Duration) -> Self {
        Self {
            state: WakerTestFutureState::Init(duration),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    fn spawn(duration: Duration, ready: Arc<AtomicBool>, waker: Waker) {
        thread::spawn(move || {
            thread::sleep(duration);
            debug!(target: "WakerTestFuture::spawn", "waking for no reason");
            waker.wake_by_ref();
            thread::sleep(duration);
            debug!(target: "WakerTestFuture::spawn", "setting ready to true");
            ready.store(true, Ordering::SeqCst);
            debug!(target: "WakerTestFuture::spawn", "waking because done");
            waker.wake();
        });
    }
}

#[derive(Debug)]
enum WakerTestFutureState {
    Init(Duration),
    Spawned,
}

impl Future for WakerTestFuture {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        debug!(target: "WakerTestFuture", "polled");
        let projected = self.project();
        loop {
            match projected.state {
                WakerTestFutureState::Init(duration) => {
                    debug!(target: "WakerTestFuture", "spawning");

                    info!("Cloning a new waker");
                    let new_waker = cx.waker().clone();

                    WakerTestFuture::spawn(*duration, projected.ready.clone(), new_waker);
                    *projected.state = WakerTestFutureState::Spawned;
                }
                WakerTestFutureState::Spawned => {
                    debug!(target: "WakerTestFuture", "checking if ready");
                    let ready = projected.ready.load(Ordering::SeqCst);
                    debug!(
                        target: "WakerTestFuture",
                        ready = ready,
                        "ready check completed"
                    );
                    if ready {
                        return std::task::Poll::Ready(());
                    } else {
                        return std::task::Poll::Pending;
                    }
                }
            }
        }
    }
}
