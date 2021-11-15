use std::time::{Duration, Instant};
use tracing::info;
use tracing_subscriber::prelude::*;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let runtime = guillotine::runtime::Runtime::new()?;

    runtime.block_on(main_future())?;
    Ok(())
}

async fn main_future() -> Result<()> {
    let start = Instant::now();

    info!("Spawning tasks");
    let mut tasks = Vec::new();
    for i in 0..10 {
        let task = guillotine::task::spawn(task(i));
        tasks.push(task);
    }

    info!(
        elapsed = ?Instant::now().duration_since(start),
        "Tasks spawned",
    );

    info!("Waiting for tasks");

    for task in tasks {
        task.await?;
    }

    info!(
        elapsed = ?Instant::now().duration_since(start),
        "All tasks completed",
    );

    Ok(())
}

async fn task(i: usize) -> Result<(), Box<dyn std::error::Error>> {
    info!(%i, "Spawned");
    guillotine::time::sleep(Duration::from_secs(1)).await?;
    info!(%i, "Completed");
    Ok(())
}
