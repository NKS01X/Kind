mod avl;
mod debug;
pub mod server;

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50051".parse()?;
    server::run_server(addr).await?;
    Ok(())
}
