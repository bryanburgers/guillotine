use tracing::info;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let runtime = guillotine::runtime::Runtime::new()?;

    let future = async {
        let socket = std::net::UdpSocket::bind("0.0.0.0:7000")?;
        let socket = guillotine::net::UdpSocket::new(socket)?;

        let mut buf = [0_u8; 1024];
        loop {
            info!("Listening...");
            let (size, addr) = socket.recv_from(&mut buf[..]).await?;
            let data = &buf[..size];
            info!(data = ?data, "Got data!");
            let sent_back = socket.send_to(data, addr).await?;
            info!(sent_back = sent_back, "Sent back!");
        }

        #[allow(unreachable_code)]
        Result::<(), Box<dyn std::error::Error>>::Ok(())
    };
    runtime.block_on(future)?;
    Ok(())
}
