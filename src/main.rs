use anyhow::Result;
use arrpc_rs::{bridge::BridgeServer, server::Server};
use owo_colors::OwoColorize;
use tokio::{select, signal};
use tracing::{info, Level};
use tracing_subscriber::{fmt::time, FmtSubscriber};

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_timer(time::ChronoLocal::new("%H:%M:%S".into()))
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    info!("{}", "arRPC Started".magenta().bold());
    let bridge = BridgeServer::try_bind().await?;
    let mut server = Server::try_bind().await?;
    loop {
        select! {
            activity = server.recv() => {
                if let Some(activity) = activity {
                    bridge.send_activity(activity).await?;
                } else {
                    break;
                };
            }
            _ = signal::ctrl_c() => {
                // Just to make sure the ^C doesn't gets printed
                print!("\r");
                info!("Shutting Down");
                bridge.close().await?;
                break;
            }
        }
    }
    Ok(())
}
