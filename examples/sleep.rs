use std::time::Duration;

use tracing::info;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let runtime = guillotine::runtime::Runtime::new()?;

    let future = async {
        info!("before sleep");
        guillotine::time::sleep(Duration::from_secs(1)).await?;
        info!("after sleep");

        let mut interval = guillotine::time::interval(Duration::from_secs(1))?;

        for _ in 0..5 {
            let r = interval.tick().await?;
            info!(r = r, "after tick")
        }

        Result::<(), Box<dyn std::error::Error>>::Ok(())
    };
    runtime.block_on(future)?;
    Ok(())
}
