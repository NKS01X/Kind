use std::sync::{Arc, RwLock};
use tonic::{transport::Server, Request, Response, Status};

use crate::avl::AVLTree;

pub mod kind_pb {
    tonic::include_proto!("kind");
}

use kind_pb::kind_service_server::{KindService, KindServiceServer};
use kind_pb::{
    DeleteRequest, DeleteResponse, GetRequest, PutRequest, PutResponse, RangeScanRequest,
    RangeScanResponse, Record,
};

#[derive(Clone, Debug)]
pub struct DbRecord {
    pub key: String,
    pub value: Vec<u8>,
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

pub struct KindServerImpl {
    tree: Arc<RwLock<AVLTree<DbRecord>>>,
}

impl KindServerImpl {
    pub fn new() -> Self {
        Self {
            tree: Arc::new(RwLock::new(AVLTree::new())),
        }
    }
}

#[tonic::async_trait]
impl KindService for KindServerImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<Record>, Status> {
        let req = request.into_inner();
        let target = DbRecord {
            key: req.key,
            value: vec![],
        };
        let tree = self.tree.read().unwrap();
        match tree.get(&target) {
            Some(record) => Ok(Response::new(Record {
                key: record.key.clone(),
                value: record.value.clone(),
            })),
            None => Err(Status::not_found("Key not found")),
        }
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();
        let record = DbRecord {
            key: req.key,
            value: req.value,
        };
        let mut tree = self.tree.write().unwrap();
        tree.insert(record);
        Ok(Response::new(PutResponse { success: true }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let target = DbRecord {
            key: req.key,
            value: vec![],
        };
        let mut tree = self.tree.write().unwrap();
        // Check if exists
        let exists = tree.get(&target).is_some();
        if exists {
            tree.delete(&target);
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
        let lo = DbRecord {
            key: req.lo,
            value: vec![],
        };
        let hi = DbRecord {
            key: req.hi,
            value: vec![],
        };
        
        let tree = self.tree.read().unwrap();
        let results = tree.range(&lo, &hi);
        
        let records = results
            .into_iter()
            .map(|r| Record {
                key: r.key,
                value: r.value,
            })
            .collect();
            
        Ok(Response::new(RangeScanResponse { records }))
    }
}

pub async fn run_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let server = KindServerImpl::new();
    println!("Kind DB listening on {}", addr);
    
    Server::builder()
        .add_service(KindServiceServer::new(server))
        .serve(addr)
        .await?;
        
    Ok(())
}
#[cfg(test)]
mod tests {
    use crate::avl::AVLTree;
    use crate::server::DbRecord;

    #[test]
    fn test_avl() {
        let mut tree = AVLTree::new();
        tree.insert(DbRecord { key: "b".to_string(), value: vec![1] });
        tree.insert(DbRecord { key: "a".to_string(), value: vec![2] });
        tree.insert(DbRecord { key: "c".to_string(), value: vec![3] });

        assert_eq!(tree.get(&DbRecord { key: "b".to_string(), value: vec![] }).unwrap().value, vec![1]);
        
        tree.delete(&DbRecord { key: "b".to_string(), value: vec![] });
        assert!(tree.get(&DbRecord { key: "b".to_string(), value: vec![] }).is_none());

        let range = tree.range(&DbRecord { key: "a".to_string(), value: vec![] }, &DbRecord { key: "c".to_string(), value: vec![] });
        assert_eq!(range.len(), 2);
    }
}
