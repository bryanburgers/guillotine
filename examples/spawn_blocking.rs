use std::time::Duration;
use tracing::info;
use tracing_subscriber::prelude::*;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let current_thread_id = std::thread::current().id();
    info!(message = "outside runtime", thread_id = ?current_thread_id);

    let future = async {
        let current_thread_id = std::thread::current().id();
        info!(message = "inside runtime", thread_id = ?current_thread_id);

        let handle1 = guillotine::task::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(100));
            std::thread::current().id()
        });
        let handle2 = guillotine::task::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(100));
            std::thread::current().id()
        });

        let handle_1_thread_id = handle1.await;
        let handle_2_thread_id = handle2.await;
        info!(message = "task 1", thread_id = ?handle_1_thread_id);
        info!(message = "task 2", thread_id = ?handle_2_thread_id);
    };

    let runtime = guillotine::runtime::Runtime::new()?;

    runtime.block_on(future);
    Ok(())
}
