use tracing::{info, info_span};
use tracing_subscriber::prelude::*;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let runtime = guillotine::runtime::Runtime::new()?;

    let future = listener();
    runtime.block_on(future)?;
    Ok(())
}

async fn listener() -> Result<()> {
    let listener = std::net::TcpListener::bind("0.0.0.0:7000")?;
    let listener = guillotine::net::TcpListener::new(listener)?;

    let mut connection_id = 0;
    loop {
        info!("Listening...");
        connection_id += 1;
        let (stream, addr) = listener.accept().await?;
        info!(id = connection_id, %addr, "Got connection");
        let _handle = guillotine::task::spawn(connection(connection_id, stream));
    }
}

async fn connection(id: u64, mut stream: guillotine::net::TcpStream) -> Result<()> {
    let mut buf = [0_u8; 1024];
    let _guard = info_span!("connection", id = id).entered();
    loop {
        let read = stream.read(&mut buf).await?;
        info!(read = read);
        if read == 0 {
            break;
        }
        let written = stream.write(&buf[0..read]).await?;
        info!(written = written);
    }

    info!("disconnected");
    Ok(())
}
