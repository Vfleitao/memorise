use memorize_core::{SetError, Store};
use memorize_proto::memorize_server::Memorize;
use memorize_proto::{
    ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse,
    GetRequest, GetResponse, KeysRequest, KeysResponse, SetRequest, SetResponse,
};
use tonic::{Request, Response, Status};

/// Maximum allowed key length (1 KB)
const MAX_KEY_LENGTH: usize = 1024;

/// Maximum allowed value length (1 MB)
const MAX_VALUE_LENGTH: usize = 1024 * 1024;

/// Default limit for keys listing
const DEFAULT_KEYS_LIMIT: u32 = 10000;

/// Truncates a key for safe logging (prevents leaking sensitive key data)
fn truncate_key_for_log(key: &str) -> String {
    const MAX_LOG_LEN: usize = 16;
    if key.len() <= MAX_LOG_LEN {
        key.to_string()
    } else {
        format!("{}...", &key[..MAX_LOG_LEN])
    }
}

/// Validates that a key is within size limits
fn validate_key(key: &str) -> Result<(), Status> {
    if key.is_empty() {
        return Err(Status::invalid_argument("Key cannot be empty"));
    }
    if key.len() > MAX_KEY_LENGTH {
        return Err(Status::invalid_argument(format!(
            "Key exceeds maximum length of {} bytes",
            MAX_KEY_LENGTH
        )));
    }
    Ok(())
}

/// Validates that a value is within size limits
fn validate_value(value: &str) -> Result<(), Status> {
    if value.len() > MAX_VALUE_LENGTH {
        return Err(Status::invalid_argument(format!(
            "Value exceeds maximum length of {} bytes",
            MAX_VALUE_LENGTH
        )));
    }
    Ok(())
}

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
        validate_key(key)?;
        tracing::debug!("GET {}", truncate_key_for_log(key));

        let value = self.store.get(key);
        Ok(Response::new(GetResponse { value }))
    }

    async fn set(&self, request: Request<SetRequest>) -> Result<Response<SetResponse>, Status> {
        let req = request.get_ref();
        validate_key(&req.key)?;
        validate_value(&req.value)?;
        
        let ttl_display = if req.ttl_seconds == 0 { "never".to_string() } else { format!("{}s", req.ttl_seconds) };
        tracing::debug!("SET {} (ttl: {})", truncate_key_for_log(&req.key), ttl_display);

        match self.store.set(&req.key, &req.value, req.ttl_seconds) {
            Ok(()) => Ok(Response::new(SetResponse { success: true })),
            Err(SetError::StorageFull { current_bytes, max_bytes }) => {
                tracing::warn!(
                    "Storage full: {} bytes used of {} bytes max",
                    current_bytes,
                    max_bytes
                );
                Err(Status::resource_exhausted(format!(
                    "Storage capacity exceeded ({} of {} bytes used)",
                    current_bytes, max_bytes
                )))
            }
        }
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let key = &request.get_ref().key;
        validate_key(key)?;
        tracing::debug!("DELETE {}", truncate_key_for_log(key));

        let deleted = self.store.delete(key);

        Ok(Response::new(DeleteResponse { deleted }))
    }

    async fn keys(&self, request: Request<KeysRequest>) -> Result<Response<KeysResponse>, Status> {
        let req = request.get_ref();
        let limit = if req.limit == 0 { DEFAULT_KEYS_LIMIT } else { req.limit };
        tracing::debug!("KEYS (limit: {})", limit);

        let keys = self.store.keys_with_limit(Some(limit as usize));

        Ok(Response::new(KeysResponse { keys }))
    }

    async fn contains(
        &self,
        request: Request<ContainsRequest>,
    ) -> Result<Response<ContainsResponse>, Status> {
        let key = &request.get_ref().key;
        validate_key(key)?;
        tracing::debug!("CONTAINS {}", truncate_key_for_log(key));

        let exists = self.store.contains_key(key);

        Ok(Response::new(ContainsResponse { exists }))
    }
}
