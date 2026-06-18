pub mod server;
pub mod cache;
pub mod schema;
pub mod wal;

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50051".parse()?;
    
    let snapshot_path = std::env::var("KIND_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "kind_snapshot.json".to_string());
    
    let schema_path = std::env::var("KIND_SCHEMA_PATH")
        .unwrap_or_else(|_| "schema.ksl".to_string());
        
    let wal_path = std::env::var("KIND_WAL_PATH")
        .unwrap_or_else(|_| "kind.wal".to_string());
        
    server::run_server(addr, Some(snapshot_path), Some(schema_path), Some(wal_path)).await?;
    Ok(())
}
