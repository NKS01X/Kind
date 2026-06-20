use crate::server::KindServerImpl;
use crate::server::kind_pb::{kind_service_client::KindServiceClient, SyncRequest};
use tonic::Request;
use std::sync::Arc;
use tokio_stream::StreamExt;
use std::sync::atomic::Ordering;
use crate::wal::{WalCommand, WalEnvelope};

pub async fn run_replica_loop(server: Arc<KindServerImpl>, leader_addr: String) {
    let host = if leader_addr.starts_with("http") {
        leader_addr.clone()
    } else {
        format!("http://{}", leader_addr)
    };

    loop {
        println!("[REPLICA] Attempting to connect to leader at {}...", host);
        match KindServiceClient::connect(host.clone()).await {
            Ok(mut client) => {
                let last_known_tx_id = server.global_version.load(Ordering::SeqCst);
                println!("[REPLICA] Connected. Syncing from tx_id: {}", last_known_tx_id);
                
                let req = Request::new(SyncRequest {
                    last_known_tx_id,
                });
                
                match client.sync(req).await {
                    Ok(response) => {
                        let mut stream = response.into_inner();
                        while let Some(res) = stream.next().await {
                            match res {
                                Ok(payload) => {
                                    if let Some(p) = payload.payload {
                                        match p {
                                            crate::server::kind_pb::sync_payload::Payload::Snapshot(chunk) => {
                                                for rec in chunk.records {
                                                    server.tree.insert(rec.key.clone(), crate::server::DbRecord {
                                                        value: rec.value.clone(),
                                                        expires_at: rec.expires_at,
                                                        version: chunk.max_tx_id,
                                                    });
                                                    server.index_record(&rec.key, &rec.value);
                                                }
                                                server.global_version.store(chunk.max_tx_id, Ordering::SeqCst);
                                            }
                                            crate::server::kind_pb::sync_payload::Payload::WalRecord(wal_rec) => {
                                                let tx_id = wal_rec.tx_id;
                                                let current_tx_id = server.global_version.load(Ordering::SeqCst);
                                                
                                                if tx_id <= current_tx_id {
                                                    // Duplicate, ignore
                                                    continue;
                                                }
                                                
                                                if tx_id != current_tx_id + 1 && current_tx_id != 0 {
                                                    println!("[REPLICA] FATAL: Missed tx_id! Expected {}, got {}. Reconnecting...", current_tx_id + 1, tx_id);
                                                    break; // Break the stream and reconnect to get snapshot/seek
                                                }
                                                
                                                // Deserialize the wal payload (it's JSON)
                                                if let Ok(json_str) = String::from_utf8(wal_rec.payload) {
                                                    if let Ok(envelope) = serde_json::from_str::<WalEnvelope>(&json_str) {
                                                        apply_command(&server, tx_id, envelope.c);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("[REPLICA] Stream error: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("[REPLICA] Sync failed: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("[REPLICA] Failed to connect: {:?}", e);
            }
        }
        
        // Wait before reconnecting
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

fn apply_command(server: &Arc<KindServerImpl>, tx_id: u64, cmd: WalCommand) {
    let mut lock = server.wal.lock().unwrap();
    match cmd {
        WalCommand::Put { key, value, expires_at } => {
            if let Some(old) = server.tree.get(&key) {
                server.remove_record_from_index(&key, &old.value().value);
            }
            server.index_record(&key, &value);
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(tx_id, &WalCommand::Put { key: key.clone(), value: value.clone(), expires_at });
            }
            server.cache.invalidate(&key);
            server.tree.insert(key, crate::server::DbRecord { value, expires_at, version: tx_id });
        }
        WalCommand::Delete { key } => {
            if let Some(old) = server.tree.get(&key) {
                server.remove_record_from_index(&key, &old.value().value);
            }
            server.tree.remove(&key);
            server.cache.invalidate(&key);
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(tx_id, &WalCommand::Delete { key });
            }
        }
        WalCommand::TxCommit(cmds) => {
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(tx_id, &WalCommand::TxCommit(cmds.clone()));
            }
            for subcmd in cmds {
                match subcmd {
                    WalCommand::Put { key, value, expires_at } => {
                        if let Some(old) = server.tree.get(&key) {
                            server.remove_record_from_index(&key, &old.value().value);
                        }
                        server.index_record(&key, &value);
                        server.cache.invalidate(&key);
                        server.tree.insert(key, crate::server::DbRecord { value, expires_at, version: tx_id });
                    }
                    WalCommand::Delete { key } => {
                        if let Some(old) = server.tree.get(&key) {
                            server.remove_record_from_index(&key, &old.value().value);
                        }
                        server.tree.remove(&key);
                        server.cache.invalidate(&key);
                    }
                    _ => {}
                }
            }
        }
    }
    server.global_version.store(tx_id, Ordering::SeqCst);
}
