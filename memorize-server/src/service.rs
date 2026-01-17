use memorize_core::Store;
use memorize_proto::memorize_server::Memorize;
use memorize_proto::{
    ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse,
    GetRequest, GetResponse, KeysRequest, KeysResponse, SetRequest, SetResponse,
};
use tonic::{Request, Response, Status};

/// The gRPC service implementation
pub struct MemorizeService {
    store: Store,
}

impl MemorizeService {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl Memorize for MemorizeService {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("GET {}", key);

        let value = self.store.get(key);
        Ok(Response::new(GetResponse { value }))
    }

    async fn set(&self, request: Request<SetRequest>) -> Result<Response<SetResponse>, Status> {
        let req = request.get_ref();
        tracing::debug!("SET {} (ttl: {}s)", req.key, req.ttl_seconds);

        self.store.set(&req.key, &req.value, req.ttl_seconds);

        Ok(Response::new(SetResponse { success: true }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("DELETE {}", key);

        let deleted = self.store.delete(key);

        Ok(Response::new(DeleteResponse { deleted }))
    }

    async fn keys(&self, _request: Request<KeysRequest>) -> Result<Response<KeysResponse>, Status> {
        tracing::debug!("KEYS");

        let keys = self.store.keys();

        Ok(Response::new(KeysResponse { keys }))
    }

    async fn contains(
        &self,
        request: Request<ContainsRequest>,
    ) -> Result<Response<ContainsResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("CONTAINS {}", key);

        let exists = self.store.contains_key(key);

        Ok(Response::new(ContainsResponse { exists }))
    }
}
