use std::sync::{Arc, RwLock};
use std::fs::{File, rename};
use std::io::{BufReader, BufWriter, BufRead};
use serde::{Serialize, Deserialize};
use tonic::{transport::Server, Request, Response, Status};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crossbeam_skiplist::{SkipMap, SkipSet};
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
    PrefixScanRequest, ScanFilter,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbRecord {
    pub value: Vec<u8>,
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub version: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub key: String,
    pub record: DbRecord,
}

impl PartialEq for DbRecord {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}
impl Eq for DbRecord {}

pub fn is_expired(expires_at: Option<u64>) -> bool {
    if let Some(exp) = expires_at {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            return now.as_millis() as u64 > exp;
        }
    }
    false
}

pub fn matches_filters(val: &[u8], filters: &[ScanFilter]) -> bool {
    if filters.is_empty() {
        return true;
    }
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(val) {
        if let Some(obj) = json.as_object() {
            for filter in filters {
                if let Some(v) = obj.get(&filter.field) {
                    let matches = match v {
                        serde_json::Value::String(s) => s == &filter.value,
                        serde_json::Value::Number(n) => {
                            if n.to_string() == filter.value {
                                true
                            } else if let (Some(f1), Ok(f2)) = (n.as_f64(), filter.value.parse::<f64>()) {
                                (f1 - f2).abs() < f64::EPSILON
                            } else {
                                false
                            }
                        }
                        serde_json::Value::Bool(b) => b.to_string() == filter.value,
                        _ => false,
                    };
                    if !matches {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            return true;
        }
    }
    false
}

#[derive(Clone)]
pub struct KindServerImpl {
    pub tree: Arc<SkipMap<String, DbRecord>>,
    pub indexes: Arc<SkipMap<String, SkipSet<String>>>,
    pub cache: Arc<Box<dyn Cache<String, Vec<u8>> + Send + Sync>>,
    pub snapshot_path: Option<String>,
    pub schema_registry: Arc<RwLock<SchemaRegistry>>,
    pub wal: Arc<Mutex<Option<Wal>>>,
    pub global_version: Arc<AtomicU64>,
}

impl KindServerImpl {
    pub fn new(snapshot_path: Option<String>, schema_path: Option<String>, wal_path: Option<String>) -> Self {
        let tree = Arc::new(SkipMap::new());
        let indexes = Arc::new(SkipMap::new());
        let cache: Box<dyn Cache<String, Vec<u8>> + Send + Sync> = Box::new(LruCache::new(1000));
        
        let mut registry = SchemaRegistry::new();
        if let Some(path) = &schema_path {
            if let Ok(sdl) = std::fs::read_to_string(path) {
                let _ = registry.load_schema(&sdl);
            }
        }
        let schema_registry = Arc::new(RwLock::new(registry));
        let mut max_ver = 0;

        let extract_keys = |key: &str, val: &[u8], reg: &SchemaRegistry| -> Vec<String> {
            let mut keys = Vec::new();
            if let Some(type_prefix) = key.split(':').next() {
                if let Some(indexed_fields) = reg.indexed_fields_for_prefix(type_prefix) {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(val) {
                        if let Some(obj) = json.as_object() {
                            if let Some(type_name) = reg.prefix_map.get(type_prefix) {
                                for field_def in indexed_fields {
                                    if let Some(v) = obj.get(&field_def.name) {
                                        let v_str = match v {
                                            serde_json::Value::String(s) => s.to_string(),
                                            serde_json::Value::Number(n) => n.to_string(),
                                            serde_json::Value::Bool(b) => b.to_string(),
                                            _ => continue,
                                        };
                                        keys.push(format!("{}:{}:{}", type_name, field_def.name, v_str));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            keys
        };

        if let Some(snap_path) = &snapshot_path {
            if let Ok(file) = File::open(snap_path) {
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if line.trim().is_empty() { continue; }
                        if let Ok(snapshot_record) = serde_json::from_str::<SnapshotRecord>(&line) {
                            if !is_expired(snapshot_record.record.expires_at) {
                                max_ver = max_ver.max(snapshot_record.record.version);
                                let keys = extract_keys(&snapshot_record.key, &snapshot_record.record.value, &*schema_registry.read().unwrap());
                                for k in keys {
                                    let index_set = match indexes.get(&k) {
                                        Some(entry) => entry,
                                        None => indexes.insert(k, SkipSet::new())
                                    };
                                    index_set.value().insert(snapshot_record.key.clone());
                                }
                                tree.insert(snapshot_record.key, snapshot_record.record);
                            }
                        }
                    }
                }
            }
        }

        if let Some(wal_path_str) = wal_path.as_ref() {
            let wal_path = Path::new(wal_path_str);
            if let Ok(commands) = Wal::read_all_commands(wal_path) {
                let registry = schema_registry.read().unwrap();
                for cmd in commands {
                    match cmd {
                        WalCommand::Put { key, value, expires_at } => {
                            if !is_expired(expires_at) {
                                let keys = extract_keys(&key, &value, &registry);
                                for k in keys {
                                    let index_set = match indexes.get(&k) {
                                        Some(entry) => entry,
                                        None => indexes.insert(k, SkipSet::new())
                                    };
                                    index_set.value().insert(key.clone());
                                }
                                max_ver += 1;
                                tree.insert(key, DbRecord { value, expires_at, version: max_ver });
                            }
                        }
                        WalCommand::Delete { key } => {
                            if let Some(old) = tree.get(&key) {
                                let old_keys = extract_keys(&key, &old.value().value, &registry);
                                for k in old_keys {
                                    if let Some(index_set) = indexes.get(&k) {
                                        index_set.value().remove(&key);
                                    }
                                }
                            }
                            tree.remove(&key);
                            max_ver += 1;
                        }
                        WalCommand::TxCommit(cmds) => {
                            for subcmd in cmds {
                                match subcmd {
                                    WalCommand::Put { key, value, expires_at } => {
                                        if !is_expired(expires_at) {
                                            let keys = extract_keys(&key, &value, &registry);
                                            for k in keys {
                                                let index_set = match indexes.get(&k) {
                                                    Some(entry) => entry,
                                                    None => indexes.insert(k, SkipSet::new())
                                                };
                                                index_set.value().insert(key.clone());
                                            }
                                            tree.insert(key, DbRecord { value, expires_at, version: max_ver + 1 });
                                        }
                                    }
                                    WalCommand::Delete { key } => {
                                        if let Some(old) = tree.get(&key) {
                                            let old_keys = extract_keys(&key, &old.value().value, &registry);
                                            for k in old_keys {
                                                if let Some(index_set) = indexes.get(&k) {
                                                    index_set.value().remove(&key);
                                                }
                                            }
                                        }
                                        tree.remove(&key);
                                    }
                                    _ => {}
                                }
                            }
                            max_ver += 1;
                        }
                    }
                }
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
            cache: Arc::new(cache),
            snapshot_path,
            schema_registry,
            wal: Arc::new(Mutex::new(wal)),
            global_version: Arc::new(AtomicU64::new(max_ver)),
        }
    }

    pub fn extract_index_keys(&self, key: &str, val: &[u8]) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(type_prefix) = key.split(':').next() {
            let registry = self.schema_registry.read().unwrap();
            if let Some(indexed_fields) = registry.indexed_fields_for_prefix(type_prefix) {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(val) {
                    if let Some(obj) = json.as_object() {
                        if let Some(type_name) = registry.prefix_map.get(type_prefix) {
                            for field_def in indexed_fields {
                                if let Some(v) = obj.get(&field_def.name) {
                                    let v_str = match v {
                                        serde_json::Value::String(s) => s.to_string(),
                                        serde_json::Value::Number(n) => n.to_string(),
                                        serde_json::Value::Bool(b) => b.to_string(),
                                        _ => continue,
                                    };
                                    keys.push(format!("{}:{}:{}", type_name, field_def.name, v_str));
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
                Err(_) => return,
            };
            
            let mut writer = BufWriter::new(file);
            use std::io::Write;
            for entry in self.tree.iter() {
                if !is_expired(entry.value().expires_at) {
                    let record_json = serde_json::to_string(&SnapshotRecord {
                        key: entry.key().clone(),
                        record: entry.value().clone(),
                    }).unwrap();
                    let _ = writeln!(writer, "{}", record_json);
                }
            }
            
            if let Err(_) = rename(&temp_path, path) { return; }
            
            if let Some(wal) = wal_lock.as_mut() {
                let _ = wal.truncate();
            }
        }
    }

    pub fn begin_transaction(&self) -> TxHandle<'_> {
        let snapshot_version = self.global_version.load(Ordering::SeqCst);
        TxHandle {
            server: self,
            write_set: HashMap::new(),
            snapshot_version,
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
    NotFound,
    Conflict,
    CommitFailed(String),
}

pub struct TxHandle<'a> {
    pub server: &'a KindServerImpl,
    pub write_set: HashMap<String, Option<DbRecord>>,
    pub snapshot_version: u64,
}

impl<'a> TxHandle<'a> {
    pub fn put(&mut self, key: String, value: Vec<u8>, ttl_ms: Option<u64>) {
        let expires_at = ttl_ms.map(|ms| {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms
        });
        self.write_set.insert(key, Some(DbRecord { value, expires_at, version: 0 }));
    }

    pub fn delete(&mut self, key: String) {
        self.write_set.insert(key, None);
    }

    pub fn commit(self) -> Result<(), DbError> {
        let mut lock = self.server.wal.lock().map_err(|_| DbError::CommitFailed("Lock poisoned".into()))?;
        
        for (k, _) in self.write_set.iter() {
            if let Some(current) = self.server.tree.get(k) {
                if current.value().version > self.snapshot_version {
                    return Err(DbError::Conflict);
                }
            }
        }

        let new_version = self.server.global_version.fetch_add(1, Ordering::SeqCst) + 1;
        let mut pending = Vec::new();
        for (k, v) in self.write_set.iter() {
            if let Some(rec) = v {
                pending.push(WalCommand::Put { key: k.clone(), value: rec.value.clone(), expires_at: rec.expires_at });
            } else {
                pending.push(WalCommand::Delete { key: k.clone() });
            }
        }

        if let Some(wal) = lock.as_mut() {
            let _ = wal.append(&WalCommand::TxCommit(pending));
        }

        for (k, v) in self.write_set {
            if let Some(mut rec) = v {
                if let Some(old) = self.server.tree.get(&k) {
                    self.server.remove_record_from_index(&k, &old.value().value);
                }
                rec.version = new_version;
                self.server.tree.insert(k.clone(), rec.clone());
                self.server.index_record(&k, &rec.value);
            } else {
                if let Some(old) = self.server.tree.get(&k) {
                    self.server.remove_record_from_index(&k, &old.value().value);
                }
                self.server.tree.remove(&k);
            }
            self.server.cache.invalidate(&k);
        }
        Ok(())
    }

    pub fn rollback(self) {}
}

#[tonic::async_trait]
impl KindService for KindServerImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<Record>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        
        if let Some(val) = self.cache.get(&key) {
            return Ok(Response::new(Record { key: key.clone(), value: val, expires_at: None }));
        }

        match self.tree.get(&key) {
            Some(entry) => {
                let rec = entry.value();
                if is_expired(rec.expires_at) {
                    self.remove_record_from_index(&key, &rec.value);
                    self.tree.remove(&key);
                    return Err(Status::not_found("Key not found or expired"));
                }
                Ok(Response::new(Record { key: entry.key().clone(), value: rec.value.clone(), expires_at: rec.expires_at }))
            },
            None => Err(Status::not_found("Key not found")),
        }
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        let val = req.value.clone();
        let expires_at = req.ttl_ms.map(|ms| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms);

        if let Some(old) = self.tree.get(&key) {
            self.remove_record_from_index(&key, &old.value().value);
        }
        self.index_record(&key, &val);
        
        if expires_at.is_none() { self.cache.put(key.clone(), val.clone()); }
        
        let new_version = self.global_version.fetch_add(1, Ordering::SeqCst) + 1;
        
        if let Ok(mut lock) = self.wal.lock() {
            if let Some(wal) = lock.as_mut() {
                let _ = wal.append(&WalCommand::Put { key: key.clone(), value: val.clone(), expires_at });
            }
        }
        self.tree.insert(key, DbRecord { value: val, expires_at, version: new_version });
        
        Ok(Response::new(PutResponse { success: true }))
    }

    async fn delete(&self, request: Request<DeleteRequest>) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let key = req.key.clone();
        
        if let Some(old) = self.tree.get(&key) {
            self.remove_record_from_index(&key, &old.value().value);
            self.tree.remove(&key);
            self.cache.invalidate(&key);
            let _ = self.global_version.fetch_add(1, Ordering::SeqCst) + 1;
            
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

    async fn range_scan(&self, request: Request<RangeScanRequest>) -> Result<Response<RangeScanResponse>, Status> {
        let req = request.into_inner();
        let mut records = Vec::new();
        for e in self.tree.range(req.lo..=req.hi) {
            let rec = e.value();
            if !is_expired(rec.expires_at) {
                if matches_filters(&rec.value, &req.filters) {
                    records.push(Record { key: e.key().clone(), value: rec.value.clone(), expires_at: rec.expires_at });
                }
            }
        }
        Ok(Response::new(RangeScanResponse { records }))
    }

    async fn prefix_scan(&self, request: Request<PrefixScanRequest>) -> Result<Response<RangeScanResponse>, Status> {
        let req = request.into_inner();
        let mut records = Vec::new();
        for e in self.tree.range(req.prefix.clone()..) {
            if e.key().starts_with(&req.prefix) {
                let rec = e.value();
                if !is_expired(rec.expires_at) {
                    if matches_filters(&rec.value, &req.filters) {
                        records.push(Record { key: e.key().clone(), value: rec.value.clone(), expires_at: rec.expires_at });
                    }
                }
            } else { break; }
        }
        Ok(Response::new(RangeScanResponse { records }))
    }

    async fn query(&self, request: Request<QueryRequest>) -> Result<Response<QueryResponse>, Status> {
        let req = request.into_inner();
        let idx_key = format!("{}:{}:{}", req.schema_type, req.field, req.value);
        let mut records = Vec::new();
        if let Some(skip_set) = self.indexes.get(&idx_key) {
            let offset = req.offset.unwrap_or(0) as usize;
            let limit = req.limit.unwrap_or(100) as usize;
            for key in skip_set.value().iter().skip(offset).take(limit) {
                if let Some(tree_entry) = self.tree.get(key.value()) {
                    let rec = tree_entry.value();
                    if !is_expired(rec.expires_at) {
                        records.push(Record { key: key.value().clone(), value: rec.value.clone(), expires_at: rec.expires_at });
                    }
                }
            }
        }
        Ok(Response::new(QueryResponse { records }))
    }

    async fn cas(&self, request: Request<CasRequest>) -> Result<Response<CasResponse>, Status> {
        let req = request.into_inner();
        let key = req.key;
        let expected = req.expected_value;
        let new_val = req.new_value;
        let expires_at = req.ttl_ms.map(|ms| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + ms);

        let mut lock = match self.wal.lock() {
            Ok(l) => l,
            Err(_) => return Err(Status::internal("Lock failed")),
        };

        if let Some(entry) = self.tree.get(&key) {
            let rec = entry.value();
            if is_expired(rec.expires_at) { return Ok(Response::new(CasResponse { success: false })); }
            if rec.value == expected {
                self.remove_record_from_index(&key, &rec.value);
                self.index_record(&key, &new_val);
                if let Some(wal) = lock.as_mut() {
                    let _ = wal.append(&WalCommand::Put { key: key.clone(), value: new_val.clone(), expires_at });
                }
                self.cache.invalidate(&key);
                let new_version = self.global_version.fetch_add(1, Ordering::SeqCst) + 1;
                self.tree.insert(key, DbRecord { value: new_val, expires_at, version: new_version });
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

        let mut b_prefix = Vec::new();
        for e in server.tree.range("b".to_string()..) {
            if e.key().starts_with("b") {
                b_prefix.push(e.key().clone());
            } else {
                break;
            }
        }
        assert_eq!(b_prefix.len(), 3);
        assert_eq!(b_prefix[0], "b1");
        assert_eq!(b_prefix[2], "b3");

        let mut z_prefix = Vec::new();
        for e in server.tree.range("z".to_string()..) {
            if e.key().starts_with("z") {
                z_prefix.push(e.key().clone());
            } else {
                break;
            }
        }
        assert!(z_prefix.is_empty());

        let mut full_scan = Vec::new();
        for e in server.tree.range("a".to_string()..="z".to_string()) {
            full_scan.push(e.key().clone());
        }
        assert_eq!(full_scan.len(), 5);
    }

    #[test]
    fn test_secondary_index_query() {
        use std::io::Write;
        
        let mut temp_ksl = tempfile::NamedTempFile::new().unwrap();
        let ksl_content = r#"
            enum ContainerStatus { Running, Stopped }
            @prefix("container")
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

        server.tree.insert("container:1".to_string(), DbRecord { value: serde_json::to_vec(&c1).unwrap(), expires_at: None, version: 1 });
        server.index_record("container:1", &serde_json::to_vec(&c1).unwrap());
        
        server.tree.insert("container:2".to_string(), DbRecord { value: serde_json::to_vec(&c2).unwrap(), expires_at: None, version: 1 });
        server.index_record("container:2", &serde_json::to_vec(&c2).unwrap());
        
        server.tree.insert("container:3".to_string(), DbRecord { value: serde_json::to_vec(&c3).unwrap(), expires_at: None, version: 1 });
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
