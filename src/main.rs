pub mod server;
pub mod cache;
pub mod schema;
pub mod wal;
pub mod replica;

use std::net::SocketAddr;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut replica_of = None;
    while let Some(arg) = args.next() {
        if arg == "--replica-of" {
            replica_of = args.next();
        }
    }
    
    let is_replica = replica_of.is_some();
    if let Some(leader) = replica_of {
        env::set_var("KIND_REPLICA_OF", leader);
    }

    let addr: SocketAddr = "0.0.0.0:50051".parse()?;
    
    let snapshot_path = std::env::var("KIND_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "kind_snapshot.json".to_string());
    
    let schema_path = std::env::var("KIND_SCHEMA_PATH")
        .unwrap_or_else(|_| "schema.ksl".to_string());
        
    let wal_path = std::env::var("KIND_WAL_PATH")
        .unwrap_or_else(|_| "kind.wal".to_string());
        
    server::run_server(addr, Some(snapshot_path), Some(schema_path), Some(wal_path), is_replica).await?;
    Ok(())
}
