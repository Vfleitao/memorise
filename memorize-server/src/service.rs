use crate::auth::API_KEY_HEADER;
use memorize_core::{SetError, Store, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT};
use memorize_proto::memorize_server::Memorize;
use memorize_proto::{
    ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse,
    DeleteAllRequest, DeleteAllResponse,
    GetRequest, GetResponse, KeysRequest, KeysResponse, 
    SearchKeysRequest, SearchKeysResponse,
    SetRequest, SetResponse,
};
use tonic::{Request, Response, Status};

/// Maximum allowed key length (1 KB)
const MAX_KEY_LENGTH: usize = 1024;

/// Maximum allowed value length (1 MB)
const MAX_VALUE_LENGTH: usize = 1024 * 1024;

/// Default limit for keys listing
const DEFAULT_KEYS_LIMIT: u32 = 10000;

/// Maximum allowed prefix length for search operations
const MAX_PREFIX_LENGTH: usize = 256;

/// Maximum allowed skip value for search pagination (prevents abuse)
const MAX_SKIP: u32 = 10000;

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

    async fn delete_all(
        &self,
        request: Request<DeleteAllRequest>,
    ) -> Result<Response<DeleteAllResponse>, Status> {
        // Check if API key authentication is being used
        let has_api_key = request.metadata().get(API_KEY_HEADER).is_some();
        
        if has_api_key {
            tracing::debug!("DELETE_ALL");
        } else {
            tracing::warn!("DELETE_ALL: Destructive operation invoked without API key - consider using API key authentication in production");
        }

        let deleted_count = self.store.delete_all() as u64;

        Ok(Response::new(DeleteAllResponse { deleted_count }))
    }

    async fn keys(&self, request: Request<KeysRequest>) -> Result<Response<KeysResponse>, Status> {
        let req = request.get_ref();
        let limit = if req.limit == 0 { DEFAULT_KEYS_LIMIT } else { req.limit };
        tracing::debug!("KEYS (limit: {})", limit);

        let keys = self.store.keys_with_limit(Some(limit as usize));

        Ok(Response::new(KeysResponse { keys }))
    }

    async fn search_keys(
        &self,
        request: Request<SearchKeysRequest>,
    ) -> Result<Response<SearchKeysResponse>, Status> {
        let req = request.get_ref();
        
        // Validate prefix length
        if req.prefix.len() > MAX_PREFIX_LENGTH {
            return Err(Status::invalid_argument(format!(
                "Prefix exceeds maximum length of {} bytes",
                MAX_PREFIX_LENGTH
            )));
        }
        
        // Validate skip value to prevent abuse
        if req.skip > MAX_SKIP {
            return Err(Status::invalid_argument(format!(
                "Skip value exceeds maximum of {}",
                MAX_SKIP
            )));
        }
        
        // Handle limit: 0 = default, otherwise cap to max
        let limit = if req.limit == 0 {
            DEFAULT_SEARCH_LIMIT
        } else {
            (req.limit as usize).min(MAX_SEARCH_LIMIT)
        };
        let skip = if req.skip == 0 { None } else { Some(req.skip as usize) };
        
        tracing::debug!(
            "SEARCH_KEYS prefix={} limit={} skip={:?}",
            truncate_key_for_log(&req.prefix),
            limit,
            skip
        );

        let (keys, total_count) = self.store.search_keys(&req.prefix, Some(limit), skip);

        Ok(Response::new(SearchKeysResponse {
            keys,
            total_count: total_count as u64,
        }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use memorize_core::StoreConfig;
    use std::time::Duration;

    /// Creates a test store configuration with a long cleanup interval.
    /// 
    /// This function should only be called from within a `#[tokio::test]` context,
    /// as `Store::with_config` requires a tokio runtime for the background cleanup task.
    fn create_test_store() -> Store {
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_secs(3600));
        Store::with_config(config)
    }

    #[test]
    fn test_validate_key_empty() {
        let result = validate_key("");
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("empty"));
    }

    #[test]
    fn test_validate_key_too_long() {
        let long_key = "x".repeat(MAX_KEY_LENGTH + 1);
        let result = validate_key(&long_key);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("maximum length"));
    }

    #[test]
    fn test_validate_key_at_limit() {
        let key_at_limit = "x".repeat(MAX_KEY_LENGTH);
        let result = validate_key(&key_at_limit);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_value_too_long() {
        let long_value = "x".repeat(MAX_VALUE_LENGTH + 1);
        let result = validate_value(&long_value);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("maximum length"));
    }

    #[test]
    fn test_validate_value_at_limit() {
        let value_at_limit = "x".repeat(MAX_VALUE_LENGTH);
        let result = validate_value(&value_at_limit);
        assert!(result.is_ok());
    }

    #[test]
    fn test_truncate_key_for_log_short() {
        let short_key = "short";
        assert_eq!(truncate_key_for_log(short_key), "short");
    }

    #[test]
    fn test_truncate_key_for_log_long() {
        let long_key = "this_is_a_very_long_key_that_should_be_truncated";
        let truncated = truncate_key_for_log(long_key);
        assert_eq!(truncated, "this_is_a_very_l...");
        assert!(truncated.len() <= 19); // 16 chars + "..."
    }

    #[tokio::test]
    async fn test_search_keys_prefix_too_long() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        // Create a prefix that exceeds the limit
        let long_prefix = "x".repeat(MAX_PREFIX_LENGTH + 1);
        let request = Request::new(SearchKeysRequest {
            prefix: long_prefix,
            limit: 10,
            skip: 0,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_err());
        
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("Prefix"));
        assert!(status.message().contains("maximum length"));
        assert!(status.message().contains(&MAX_PREFIX_LENGTH.to_string()));
    }

    #[tokio::test]
    async fn test_search_keys_prefix_at_limit() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        // Create a prefix exactly at the limit - should succeed
        let prefix_at_limit = "x".repeat(MAX_PREFIX_LENGTH);
        let request = Request::new(SearchKeysRequest {
            prefix: prefix_at_limit,
            limit: 10,
            skip: 0,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_keys_empty_prefix_allowed() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        // Empty prefix should be allowed (matches all keys)
        let request = Request::new(SearchKeysRequest {
            prefix: String::new(),
            limit: 10,
            skip: 0,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_keys_limit_capped_to_max() {
        let store = create_test_store();
        
        // Add more keys than MAX_SEARCH_LIMIT
        for i in 0..300 {
            store.set(format!("key:{:03}", i), "value", 60).unwrap();
        }
        
        let service = MemorizeService::new(store);
        
        // Request a limit higher than MAX_SEARCH_LIMIT
        let request = Request::new(SearchKeysRequest {
            prefix: "key:".to_string(),
            limit: 500, // Higher than MAX_SEARCH_LIMIT (250)
            skip: 0,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_ok());
        
        let response = result.unwrap().into_inner();
        assert_eq!(response.keys.len(), MAX_SEARCH_LIMIT);
        assert_eq!(response.total_count, 300);
    }

    #[tokio::test]
    async fn test_delete_all_returns_count() {
        let store = create_test_store();
        
        // Add some entries
        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        store.set("key3", "value3", 60).unwrap();
        
        let service = MemorizeService::new(store.clone());
        
        let request = Request::new(DeleteAllRequest {});
        let result = service.delete_all(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        assert_eq!(response.deleted_count, 3);
        
        // Verify store is empty
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_all_empty_store() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        let request = Request::new(DeleteAllRequest {});
        let result = service.delete_all(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        assert_eq!(response.deleted_count, 0);
    }

    #[tokio::test]
    async fn test_delete_all_u64_conversion() {
        let store = create_test_store();
        
        // Add entries and verify the count converts correctly to u64
        for i in 0..100 {
            store.set(format!("key:{}", i), "value", 60).unwrap();
        }
        
        let service = MemorizeService::new(store);
        
        let request = Request::new(DeleteAllRequest {});
        let result = service.delete_all(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        // Verify it's a u64 and has correct value
        let count: u64 = response.deleted_count;
        assert_eq!(count, 100u64);
    }

    #[tokio::test]
    async fn test_search_keys_skip_too_large() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        // Request a skip value exceeding the maximum
        let request = Request::new(SearchKeysRequest {
            prefix: "test".to_string(),
            limit: 10,
            skip: MAX_SKIP + 1,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_err());
        
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("Skip"));
        assert!(status.message().contains("maximum"));
    }

    #[tokio::test]
    async fn test_search_keys_skip_at_limit() {
        let store = create_test_store();
        let service = MemorizeService::new(store);
        
        // Skip at exactly the limit should succeed
        let request = Request::new(SearchKeysRequest {
            prefix: "test".to_string(),
            limit: 10,
            skip: MAX_SKIP,
        });
        
        let result = service.search_keys(request).await;
        assert!(result.is_ok());
    }
}
