use std::sync::{Arc, RwLock};
use std::fs::{File, rename};
use std::io::{BufReader, BufWriter};
use serde::{Serialize, Deserialize};
use tonic::{transport::Server, Request, Response, Status};
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_skiplist::{SkipMap, SkipSet};
use std::collections::BTreeMap;
use crate::cache::{Cache, LruCache};
use crate::schema::SchemaRegistry;
use crate::wal::{Wal, WalCommand};
use std::sync::Mutex;

pub mod kind_pb {
    tonic::include_proto!("kind");
}

use kind_pb::kind_service_server::{KindService, KindServiceServer};
use kind_pb::{
    DeleteRequest, DeleteResponse, GetRequest, PutRequest, PutResponse, RangeScanRequest,
    RangeScanResponse, Record, QueryRequest, QueryResponse, CasRequest, CasResponse,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbRecord {
    pub key: String,
    pub value: Vec<u8>,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl PartialEq for DbRecord {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for DbRecord {}

impl PartialOrd for DbRecord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for DbRecord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

pub fn is_expired(expires_at: Option<u64>) -> bool {
    if let Some(exp) = expires_at {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            return now.as_millis() as u64 > exp;
        }
    }
    false
}

#[derive(Clone)]
pub struct KindServerImpl {
    pub tree: Arc<SkipMap<String, DbRecord>>,
    pub indexes: Arc<SkipMap<String, SkipSet<String>>>,
    pub cache: Arc<RwLock<Box<dyn Cache<String, Vec<u8>> + Send + Sync>>>,
    pub snapshot_path: Option<String>,
    pub schema_registry: Arc<RwLock<SchemaRegistry>>,
    pub wal: Arc<Mutex<Option<Wal>>>,
}

impl KindServerImpl {
    pub fn new(snapshot_path: Option<String>, schema_path: Option<String>, wal_path: Option<String>) -> Self {
        let tree = Arc::new(SkipMap::new());
        let indexes = Arc::new(SkipMap::new());
        let cache: Box<dyn Cache<String, Vec<u8>> + Send + Sync> = Box::new(LruCache::new(1000));
        
        let mut registry = SchemaRegistry::new();
        if let Some(path) = &schema_path {
            if let Ok(sdl) = std::fs::read_to_string(path) {
                if let Err(e) = registry.load_schema(&sdl) {
                    println!("Failed to parse schema {}: {}", path, e);
                } else {
                    println!("Loaded schema from {}", path);
                }
            } else {
                println!("Failed to read schema file {}", path);
            }
        }
        let schema_registry = Arc::new(RwLock::new(registry));

        let extract_keys = |key: &str, val: &[u8], reg: &SchemaRegistry| -> Vec<String> {
            let mut keys = Vec::new();
            if let Some(type_prefix) = key.split(':').next() {
                let schema_type = match type_prefix {
                    "container" => Some("ContainerRecord"),
                    "config" => Some("ClusterConfig"),
                    "event" => Some("ScalingEvent"),
                    _ => None,
                };
                if let Some(st) = schema_type {
                    if let Some(t_def) = reg.types.get(st) {
                        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(val) {
                            if let Some(obj) = json.as_object() {
                                for field_def in &t_def.fields {
                                    if field_def.is_indexed {
                                        if let Some(v) = obj.get(&field_def.name) {
                                            let v_str = match v {
                                                serde_json::Value::String(s) => s.to_string(),
                                                serde_json::Value::Number(n) => n.to_string(),
                                                serde_json::Value::Bool(b) => b.to_string(),
                                                _ => continue,
                                            };
                                            keys.push(format!("{}:{}:{}", st, field_def.name, v_str));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            keys
        };

        let index_rec = |key: &str, val: &[u8], reg: &SchemaRegistry| {
            for idx_key in extract_keys(key, val, reg) {
                let skip_set = match indexes.get(&idx_key) {
                    Some(entry) => entry,
                    None => indexes.insert(idx_key.clone(), SkipSet::new())
                };
                skip_set.value().insert(key.to_string());
            }
        };

        let remove_rec_index = |key: &str, val: &[u8], reg: &SchemaRegistry| {
            for idx_key in extract_keys(key, val, reg) {
                if let Some(skip_set) = indexes.get(&idx_key) {
                    skip_set.value().remove(key);
                }
            }
        };

        if let Some(path) = &snapshot_path {
            if let Ok(file) = File::open(path) {
                let reader = BufReader::new(file);
                let parsed: Result<Vec<DbRecord>, _> = serde_json::from_reader(reader);
                match parsed {
                    Ok(records) => {
                        for rec in records {
                            if !is_expired(rec.expires_at) {
                                tree.insert(rec.key.clone(), rec.clone());
                                index_rec(&rec.key, &rec.value, &*schema_registry.read().unwrap());
                            }
                        }
                        println!("Successfully loaded snapshot from {}", path);
                    }
                    Err(_) => {
                        if let Ok(file) = File::open(path) {
                            let reader = BufReader::new(file);
                            let parsed_old: Result<serde_json::Value, _> = serde_json::from_reader(reader);
                            if let Ok(val) = parsed_old {
                                fn extract_records(node: &serde_json::Value, tree: &SkipMap<String, DbRecord>) {
                                    if node.is_null() { return; }
                                    if let Some(n) = node.as_object() {
                                        if let Some(id) = n.get("id") {
                                            if let (Some(key), Some(val_arr)) = (id.get("key").and_then(|v| v.as_str()), id.get("value").and_then(|v| v.as_array())) {
                                                let value = val_arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect();
                                                tree.insert(key.to_string(), DbRecord { key: key.to_string(), value, expires_at: None });
                                            }
                                        }
                                        if let Some(left) = n.get("left") { extract_records(left, tree); }
                                        if let Some(right) = n.get("right") { extract_records(right, tree); }
                                    }
                                }
                                
                                if let Some(root) = val.get("root") {
                                    extract_records(root, &tree);
                                }
                                for entry in tree.iter() {
                                    if !is_expired(entry.value().expires_at) {
                                        index_rec(entry.key(), &entry.value().value, &*schema_registry.read().unwrap());
                                    } else {
                                        tree.remove(entry.key());
                                    }
                                }
                                println!("Successfully loaded old AVLTree snapshot from {}", path);
                            } else {
                                println!("Failed to load snapshot from {}", path);
                            }
                        }
                    }
                }
            }
        }

        if let Some(path) = &wal_path {
            if let Ok(commands) = Wal::read_all_commands(path) {
                for cmd in commands {
                    match cmd {
                        WalCommand::Put { key, value, expires_at } => {
                            if let Some(old) = tree.get(&key) {
                                remove_rec_index(&key, &old.value().value, &*schema_registry.read().unwrap());
                            }
                            if !is_expired(expires_at) {
                                tree.insert(key.clone(), DbRecord { key: key.clone(), value: value.clone(), expires_at });
                                index_rec(&key, &value, &*schema_registry.read().unwrap());
                            }
                        }
                        WalCommand::Delete { key } => {
                            if let Some(old) = tree.get(&key) {
                                remove_rec_index(&key, &old.value().value, &*schema_registry.read().unwrap());
                            }
                            tree.remove(&key);
                        }
                        WalCommand::TxCommit(cmds) => {
                            for tcmd in cmds {
                                match tcmd {
                                    WalCommand::Put { key, value, expires_at } => {
                                        if let Some(old) = tree.get(&key) {
                                            remove_rec_index(&key, &old.value().value, &*schema_registry.read().unwrap());
                                        }
                                        if !is_expired(expires_at) {
                                            tree.insert(key.clone(), DbRecord { key: key.clone(), value: value.clone(), expires_at });
                                            index_rec(&key, &value, &*schema_registry.read().unwrap());
                                        }
                                    }
                                    WalCommand::Delete { key } => {
                                        if let Some(old) = tree.get(&key) {
                                            remove_rec_index(&key, &old.value().value, &*schema_registry.read().unwrap());
                                        }
                                        tree.remove(&key);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                println!("Replayed WAL from {}", path);
            }
        }

        let wal = if let Some(path) = wal_path {
            Wal::new(path).ok()
        } else {
            None
        };

        Self {
            tree,
            indexes,
            cache: Arc::new(RwLock::new(cache)),
            snapshot_path,
            schema_registry,
            wal: Arc::new(Mutex::new(wal)),
        }
    }

    pub fn extract_index_keys(&self, key: &str, val: &[u8]) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(type_prefix) = key.split(':').next() {
            let schema_type = match type_prefix {
                "container" => Some("ContainerRecord"),
                "config" => Some("ClusterConfig"),
                "event" => Some("ScalingEvent"),
                _ => None,
            };
            if let Some(st) = schema_type {
                let registry = self.schema_registry.read().unwrap();
                if let Some(t_def) = registry.types.get(st) {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(val) {
                        if let Some(obj) = json.as_object() {
                            for field_def in &t_def.fields {
                                if field_def.is_indexed {
                                    if let Some(v) = obj.get(&field_def.name) {
                                        let v_str = match v {
                                            serde_json::Value::String(s) => s.to_string(),
                                            serde_json::Value::Number(n) => n.to_string(),
                                            serde_json::Value::Bool(b) => b.to_string(),
                                            _ => continue,
                                        };
                                        keys.push(format!("{}:{}:{}", st, field_def.name, v_str));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        keys
    }

    pub fn index_record(&self, key: &str, val: &[u8]) {
        for idx_key in self.extract_index_keys(key, val) {
            let skip_set = match self.indexes.get(&idx_key) {
                Some(entry) => entry,
                None => self.indexes.insert(idx_key.clone(), SkipSet::new())
            };
            skip_set.value().insert(key.to_string());
        }
    }

    pub fn remove_record_from_index(&self, key: &str, val: &[u8]) {
        for idx_key in self.extract_index_keys(key, val) {
            if let Some(skip_set) = self.indexes.get(&idx_key) {
                skip_set.value().remove(key);
            }
        }
    }

    pub fn save_snapshot_and_truncate_wal(&self) {
        if let Some(path) = &self.snapshot_path {
            let temp_path = format!("{}.tmp", path);
            let mut wal_lock = self.wal.lock().unwrap();
            
            let file = match File::create(&temp_path) {
                Ok(f) => f,
                Err(e) => {
                    println!("Failed to create temp snapshot file: {}", e);
                    return;
                }
            };
            
            let writer = BufWriter::new(file);
            let mut entries = Vec::new();
            for entry in self.tree.iter() {
                if !is_expired(entry.value().expires_at) {
                    entries.push(entry.value().clone());
                }
            }
            
            if let Err(e) = serde_json::to_writer(writer, &entries) {
                println!("Failed to serialize tree to snapshot: {}", e);
                return;
            }
            
            if let Err(e) = rename(&temp_path, path) {
                println!("Failed to rename temp snapshot to final path: {}", e);
                return;
            }
            
            if let Some(wal) = wal_lock.as_mut() {
                if let Err(e) = wal.truncate() {
                    println!("Failed to truncate WAL: {}", e);
                }
            }
            println!("Background snapshot completed.");
        }
    }

    pub fn begin_transaction(&self) -> TxHandle {
        TxHandle {
            server: self,
            write_set: BTreeMap::new(),
        }
    }

    pub fn db_range_scan(&self, lo: &str, hi: &str) -> Vec<DbRecord> {
        self.tree.range(lo.to_string()..=hi.to_string())
            .filter_map(|e| {
                if !is_expired(e.value().expires_at) {
                    Some(e.value().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn prefix_scan(&self, prefix: &str) -> Vec<DbRecord> {
        let mut results = Vec::new();
        for entry in self.tree.range(prefix.to_string()..) {
            if entry.key().starts_with(prefix) {
                if !is_expired(entry.value().expires_at) {
                    results.push(entry.value().clone());
                }
            } else {
                break;
            }
        }
        results
    }
}

#[derive(Debug)]
pub enum DbError {
    CommitFailed(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::CommitFailed(msg) => write!(f, "Commit failed: {}", msg),
        }
    }
}

impl std::error::Error for DbError {}

pub struct TxHandle<'a> {
    server: &'a KindServerImpl,
    write_set: BTreeMap<String, Option<DbRecord>>,
}

impl<'a> TxHandle<'a> {
    pub fn put(&mut self, key: String, value: Vec<u8>, ttl_ms: Option<u64>) {
        let expires_at = ttl_ms.map(|ms| {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms
        });
        self.write_set.insert(key.clone(), Some(DbRecord { key, value, expires_at }));
    }

    pub fn delete(&mut self, key: String) {
        self.write_set.insert(key, None);
    }

    pub fn commit(self) -> Result<(), DbError> {
        let mut pending_commands = Vec::new();
        for (k, v) in &self.write_set {
            if let Some(rec) = v {
                pending_commands.push(WalCommand::Put { key: k.clone(), value: rec.value.clone(), expires_at: rec.expires_at });
            } else {
                pending_commands.push(WalCommand::Delete { key: k.clone() });
            }
        }

        if let Ok(mut lock) = self.server.wal.lock() {
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(&WalCommand::TxCommit(pending_commands));
            }
            let mut cache = self.server.cache.write().unwrap();
            for (k, v) in self.write_set {
                if let Some(rec) = v {
                    if let Some(old) = self.server.tree.get(&k) {
                        self.server.remove_record_from_index(&k, &old.value().value);
                    }
                    self.server.tree.insert(k.clone(), rec.clone());
                    self.server.index_record(&k, &rec.value);
                } else {
                    if let Some(old) = self.server.tree.get(&k) {
                        self.server.remove_record_from_index(&k, &old.value().value);
                    }
                    self.server.tree.remove(&k);
                }
                cache.invalidate(&k);
            }
        }
        
        Ok(())
    }

    pub fn rollback(self) {}

    pub fn db_range_scan(&self, lo: &str, hi: &str) -> Vec<DbRecord> {
        let mut results = BTreeMap::new();
        for entry in self.server.tree.range(lo.to_string()..=hi.to_string()) {
            if !is_expired(entry.value().expires_at) {
                results.insert(entry.key().clone(), entry.value().clone());
            }
        }
        for (k, v) in self.write_set.range(lo.to_string()..=hi.to_string()) {
            if let Some(rec) = v {
                results.insert(k.clone(), rec.clone());
            } else {
                results.remove(k);
            }
        }
        results.into_values().collect()
    }

    pub fn prefix_scan(&self, prefix: &str) -> Vec<DbRecord> {
        let mut results = BTreeMap::new();
        for entry in self.server.tree.range(prefix.to_string()..) {
            if entry.key().starts_with(prefix) {
                if !is_expired(entry.value().expires_at) {
                    results.insert(entry.key().clone(), entry.value().clone());
                }
            } else {
                break;
            }
        }
        for (k, v) in self.write_set.range(prefix.to_string()..) {
            if k.starts_with(prefix) {
                if let Some(rec) = v {
                    results.insert(k.clone(), rec.clone());
                } else {
                    results.remove(k);
                }
            } else {
                break;
            }
        }
        results.into_values().collect()
    }
}

#[tonic::async_trait]
impl KindService for KindServerImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<Record>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        
        {
            let mut cache = self.cache.write().unwrap();
            if let Some(val) = cache.get(&key) {
                return Ok(Response::new(Record {
                    key: key.clone(),
                    value: val.clone(),
                    expires_at: None, // We don't cache items with TTL
                }));
            }
        }

        match self.tree.get(&key) {
            Some(entry) => {
                let rec = entry.value();
                if is_expired(rec.expires_at) {
                    self.remove_record_from_index(&key, &rec.value);
                    self.tree.remove(&key);
                    return Err(Status::not_found("Key not found or expired"));
                }
                let val = rec.value.clone();
                let expires_at = rec.expires_at;
                
                if expires_at.is_none() {
                    let mut cache = self.cache.write().unwrap();
                    cache.put(key.clone(), val.clone());
                }
                Ok(Response::new(Record {
                    key: entry.key().clone(),
                    value: val,
                    expires_at,
                }))
            },
            None => Err(Status::not_found("Key not found")),
        }
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        let val = req.value.clone();
        
        let expires_at = req.ttl_ms.map(|ms| {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms
        });

        if let Some(type_prefix) = key.split(':').next() {
            let schema_type = match type_prefix {
                "container" => Some("ContainerRecord"),
                "config" => Some("ClusterConfig"),
                "event" => Some("ScalingEvent"),
                _ => None,
            };

            if let Some(t) = schema_type {
                let registry = self.schema_registry.read().unwrap();
                if registry.types.contains_key(t) {
                    if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&val) {
                        if let Err(e) = registry.validate(t, &json_val) {
                            return Err(Status::invalid_argument(format!("Schema validation failed: {}", e)));
                        }
                    } else {
                        return Err(Status::invalid_argument("Value is not valid JSON, cannot validate against schema"));
                    }
                }
            }
        }

        if let Some(old) = self.tree.get(&key) {
            self.remove_record_from_index(&key, &old.value().value);
        }
        self.tree.insert(key.clone(), DbRecord { key: key.clone(), value: val.clone(), expires_at });
        self.index_record(&key, &val);
        
        if expires_at.is_none() {
            let mut cache = self.cache.write().unwrap();
            cache.put(key.clone(), val.clone());
        }
        if let Ok(mut lock) = self.wal.lock() {
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(&WalCommand::Put { key: key.clone(), value: val, expires_at });
            }
        }
        Ok(Response::new(PutResponse { success: true }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        
        if let Some(old) = self.tree.get(&key) {
            self.remove_record_from_index(&key, &old.value().value);
            self.tree.remove(&key);
            
            {
                let mut cache = self.cache.write().unwrap();
                cache.invalidate(&key);
            }
            if let Ok(mut lock) = self.wal.lock() {
                if let Some(wal) = lock.as_mut() {
                    let _ = wal.append(&WalCommand::Delete { key: key.clone() });
                }
            }
            Ok(Response::new(DeleteResponse { success: true }))
        } else {
            Ok(Response::new(DeleteResponse { success: false }))
        }
    }

    async fn range_scan(
        &self,
        request: Request<RangeScanRequest>,
    ) -> Result<Response<RangeScanResponse>, Status> {
        let req = request.into_inner();
        let mut records = Vec::new();
        
        for e in self.tree.range(req.lo..=req.hi) {
            let rec = e.value();
            if !is_expired(rec.expires_at) {
                records.push(Record {
                    key: e.key().clone(),
                    value: rec.value.clone(),
                    expires_at: rec.expires_at,
                });
            } else {
                self.remove_record_from_index(e.key(), &rec.value);
                self.tree.remove(e.key());
            }
        }
            
        Ok(Response::new(RangeScanResponse { records }))
    }

    async fn query(
        &self,
        request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        let req = request.into_inner();
        let idx_key = format!("{}:{}:{}", req.schema_type, req.field, req.value);
        let mut records = Vec::new();

        if let Some(skip_set) = self.indexes.get(&idx_key) {
            let offset = req.offset.unwrap_or(0) as usize;
            let limit = req.limit.unwrap_or(100) as usize;
            
            let mut valid_keys = Vec::new();
            for entry in skip_set.value().iter() {
                valid_keys.push(entry.value().clone());
            }
            
            for key in valid_keys.into_iter().skip(offset).take(limit) {
                if let Some(tree_entry) = self.tree.get(&key) {
                    let rec = tree_entry.value();
                    if !is_expired(rec.expires_at) {
                        records.push(Record {
                            key: key.clone(),
                            value: rec.value.clone(),
                            expires_at: rec.expires_at,
                        });
                    } else {
                        self.remove_record_from_index(&key, &rec.value);
                        self.tree.remove(&key);
                    }
                }
            }
        }

        Ok(Response::new(QueryResponse { records }))
    }

    async fn cas(
        &self,
        request: Request<CasRequest>,
    ) -> Result<Response<CasResponse>, Status> {
        let req = request.into_inner();
        let key = req.key;
        let expected = req.expected_value;
        let new_val = req.new_value;
        let expires_at = req.ttl_ms.map(|ms| {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms
        });

        let mut lock = match self.wal.lock() {
            Ok(l) => l,
            Err(_) => return Err(Status::internal("Failed to acquire WAL lock")),
        };

        if let Some(entry) = self.tree.get(&key) {
            let rec = entry.value();
            if is_expired(rec.expires_at) {
                self.remove_record_from_index(&key, &rec.value);
                self.tree.remove(&key);
                return Ok(Response::new(CasResponse { success: false })); // Expected something, found expired
            }
            if rec.value == expected {
                self.remove_record_from_index(&key, &rec.value);
                self.tree.insert(key.clone(), DbRecord { key: key.clone(), value: new_val.clone(), expires_at });
                self.index_record(&key, &new_val);
                
                if let Some(wal) = lock.as_mut() {
                    let _ = wal.append(&WalCommand::Put { key: key.clone(), value: new_val.clone(), expires_at });
                }
                
                let mut cache = self.cache.write().unwrap();
                cache.invalidate(&key);
                
                return Ok(Response::new(CasResponse { success: true }));
            }
        }

        Ok(Response::new(CasResponse { success: false }))
    }
}

pub async fn run_server(addr: std::net::SocketAddr, snapshot_path: Option<String>, schema_path: Option<String>, wal_path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let server = Arc::new(KindServerImpl::new(snapshot_path, schema_path, wal_path));
    
    // Background Snapshot & WAL Truncation
    let server_clone = server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            server_clone.save_snapshot_and_truncate_wal();
        }
    });

    // Background TTL Eviction Task
    let server_ttl = server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut to_remove = Vec::new();
            for entry in server_ttl.tree.iter() {
                let rec = entry.value();
                if is_expired(rec.expires_at) {
                    to_remove.push((entry.key().clone(), rec.value.clone()));
                }
            }
            for (key, val) in to_remove {
                server_ttl.remove_record_from_index(&key, &val);
                server_ttl.tree.remove(&key);
            }
        }
    });

    println!("Kind DB listening on {}", addr);
    
    Server::builder()
        .add_service(KindServiceServer::new((*server).clone()))
        .serve(addr)
        .await?;
        
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::server::DbRecord;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn test_transaction() {
        let server = crate::server::KindServerImpl::new(None, None, None);
        
        {
            let mut tx = server.begin_transaction();
            tx.put("key1".to_string(), vec![1], None);
            tx.commit().unwrap();
        }

        {
            let mut tx = server.begin_transaction();
            tx.put("key2".to_string(), vec![2], None);
            tx.delete("key1".to_string());
            tx.rollback();
        }

        assert!(server.tree.get("key2").is_none());
        assert_eq!(server.tree.get("key1").unwrap().value().value, vec![1]);
    }

    #[test]
    fn test_range_and_prefix_scan() {
        let server = crate::server::KindServerImpl::new(None, None, None);
        
        {
            let mut tx = server.begin_transaction();
            tx.put("a1".to_string(), vec![1], None);
            tx.put("b1".to_string(), vec![2], None);
            tx.put("b2".to_string(), vec![3], None);
            tx.put("b3".to_string(), vec![4], None);
            tx.put("c1".to_string(), vec![5], None);
            tx.commit().unwrap();
        }

        let b_prefix = server.prefix_scan("b");
        assert_eq!(b_prefix.len(), 3);
        assert_eq!(b_prefix[0].key, "b1");
        assert_eq!(b_prefix[2].key, "b3");

        let z_prefix = server.prefix_scan("z");
        assert!(z_prefix.is_empty());

        let empty_range = server.db_range_scan("d", "e");
        assert!(empty_range.is_empty());

        let full_scan = server.db_range_scan("a", "z");
        assert_eq!(full_scan.len(), 5);

        let single_match = server.db_range_scan("c", "c1");
        assert_eq!(single_match.len(), 1);
        assert_eq!(single_match[0].key, "c1");
    }

    #[test]
    fn test_secondary_index_query() {
        use std::io::Write;
        
        let mut temp_ksl = tempfile::NamedTempFile::new().unwrap();
        let ksl_content = r#"
            enum ContainerStatus { Running, Stopped }
            type ContainerRecord {
                id: String,
                image: String,
                port: U16,
                @indexed status: ContainerStatus,
                spawn_time: I64
            }
        "#;
        temp_ksl.write_all(ksl_content.as_bytes()).unwrap();

        let server = crate::server::KindServerImpl::new(None, Some(temp_ksl.path().to_str().unwrap().to_string()), None);
        
        let c1 = json!({ "id": "1", "image": "nginx", "port": 80, "status": "Running", "spawn_time": 100 });
        let c2 = json!({ "id": "2", "image": "redis", "port": 6379, "status": "Stopped", "spawn_time": 101 });
        let c3 = json!({ "id": "3", "image": "node", "port": 3000, "status": "Running", "spawn_time": 102 });

        server.tree.insert("container:1".to_string(), DbRecord { key: "container:1".to_string(), value: serde_json::to_vec(&c1).unwrap(), expires_at: None });
        server.index_record("container:1", &serde_json::to_vec(&c1).unwrap());
        
        server.tree.insert("container:2".to_string(), DbRecord { key: "container:2".to_string(), value: serde_json::to_vec(&c2).unwrap(), expires_at: None });
        server.index_record("container:2", &serde_json::to_vec(&c2).unwrap());
        
        server.tree.insert("container:3".to_string(), DbRecord { key: "container:3".to_string(), value: serde_json::to_vec(&c3).unwrap(), expires_at: None });
        server.index_record("container:3", &serde_json::to_vec(&c3).unwrap());

        let idx_key = "ContainerRecord:status:Running";
        let skip_set = server.indexes.get(idx_key).unwrap();
        
        let mut running_keys: Vec<String> = skip_set.value().iter().map(|e| e.value().clone()).collect();
        running_keys.sort();
        
        assert_eq!(running_keys.len(), 2);
        assert_eq!(running_keys[0], "container:1");
        assert_eq!(running_keys[1], "container:3");
    }

    #[test]
    fn test_ttl_eviction() {
        let server = crate::server::KindServerImpl::new(None, None, None);
        
        {
            let mut tx = server.begin_transaction();
            tx.put("key1".to_string(), vec![1], Some(0)); // Expires immediately
            tx.put("key2".to_string(), vec![2], Some(10000)); // Expires in 10s
            tx.commit().unwrap();
        }

        std::thread::sleep(Duration::from_millis(10));
        
        // key1 should be expired
        assert!(crate::server::is_expired(server.tree.get("key1").unwrap().value().expires_at));
        
        // key2 should be valid
        assert!(!crate::server::is_expired(server.tree.get("key2").unwrap().value().expires_at));
    }
}
