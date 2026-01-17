//! # Memorize Client
//!
//! A high-level Rust client for the Memorize in-memory cache service.
//!
//! This crate provides a simple, ergonomic API for interacting with a Memorize server,
//! hiding the underlying gRPC complexity.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use memorize_client::MemorizeClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), memorize_client::Error> {
//!     // Connect to the server
//!     let client = MemorizeClient::connect("http://localhost:50051").await?;
//!
//!     // Store a value with 5-minute TTL
//!     client.set("my-key", "my-value", Some(300)).await?;
//!
//!     // Retrieve the value
//!     if let Some(value) = client.get("my-key").await? {
//!         println!("Got: {}", value);
//!     }
//!
//!     // Delete the key
//!     client.delete("my-key").await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## With Authentication
//!
//! ```rust,no_run
//! use memorize_client::{MemorizeClient, MemorizeClientOptions};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), memorize_client::Error> {
//!     let options = MemorizeClientOptions::new("http://localhost:50051")
//!         .with_api_key("your-secret-key");
//!
//!     let client = MemorizeClient::with_options(options).await?;
//!     client.set("key", "value", None).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## JSON Serialization (requires `json` feature)
//!
//! ```rust,ignore
//! use memorize_client::MemorizeClient;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct User {
//!     name: String,
//!     age: u32,
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), memorize_client::Error> {
//!     let client = MemorizeClient::connect("http://localhost:50051").await?;
//!
//!     let user = User { name: "Alice".into(), age: 30 };
//!     client.set_json("user:1", &user, Some(3600)).await?;
//!
//!     let retrieved: Option<User> = client.get_json("user:1").await?;
//!     Ok(())
//! }
//! ```

mod error;
mod options;

pub use error::Error;
pub use options::MemorizeClientOptions;

use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

// Include the generated protobuf code
pub mod proto {
    tonic::include_proto!("memorize");
}

use proto::memorize_client::MemorizeClient as GrpcClient;
use proto::{ContainsRequest, DeleteRequest, GetRequest, KeysRequest, SearchKeysRequest, SetRequest};

/// API key interceptor that adds authentication header to all requests
#[derive(Clone)]
struct ApiKeyInterceptor {
    api_key: Option<String>,
}

impl Interceptor for ApiKeyInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if let Some(ref key) = self.api_key {
            let value = MetadataValue::try_from(key)
                .map_err(|_| Status::internal("Invalid API key format"))?;
            req.metadata_mut().insert("x-api-key", value);
        }
        Ok(req)
    }
}

type InterceptedClient =
    GrpcClient<tonic::service::interceptor::InterceptedService<Channel, ApiKeyInterceptor>>;

/// A high-level client for the Memorize cache service.
///
/// This client handles connection management, authentication, and provides
/// a simple async API for cache operations.
///
/// The client can be cloned cheaply - the underlying gRPC channel is reference-counted
/// and handles its own connection pooling and concurrency.
#[derive(Clone)]
pub struct MemorizeClient {
    inner: InterceptedClient,
}

impl MemorizeClient {
    /// Connect to a Memorize server without authentication.
    ///
    /// # Arguments
    /// * `url` - The server URL (e.g., "http://localhost:50051")
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(url: &str) -> Result<Self, Error> {
        Self::with_options(MemorizeClientOptions::new(url)).await
    }

    /// Connect to a Memorize server with custom options.
    ///
    /// # Arguments
    /// * `options` - Connection options including URL and optional API key
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::{MemorizeClient, MemorizeClientOptions};
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// let options = MemorizeClientOptions::new("http://localhost:50051")
    ///     .with_api_key("secret-key");
    /// let client = MemorizeClient::with_options(options).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn with_options(options: MemorizeClientOptions) -> Result<Self, Error> {
        let channel = Channel::from_shared(options.url.clone())
            .map_err(|e| Error::Connection(e.to_string()))?
            .connect()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        let interceptor = ApiKeyInterceptor {
            api_key: options.api_key,
        };
        let client = GrpcClient::with_interceptor(channel, interceptor);

        Ok(Self { inner: client })
    }

    /// Store a value in the cache.
    ///
    /// # Arguments
    /// * `key` - The cache key
    /// * `value` - The value to store
    /// * `ttl_seconds` - Optional time-to-live in seconds. If None or Some(0), the entry never expires.
    ///
    /// # Errors
    /// * `Error::StorageFull` - Server storage capacity has been exceeded
    /// * `Error::Grpc` - Other gRPC errors (auth failure, invalid arguments, etc.)
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// // Store with 5-minute TTL
    /// client.set("key", "value", Some(300)).await?;
    ///
    /// // Store without expiration (never expires)
    /// client.set("permanent", "value", None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
        ttl_seconds: Option<u64>,
    ) -> Result<(), Error> {
        let mut client = self.inner.clone();
        client
            .set(SetRequest {
                key: key.into(),
                value: value.into(),
                // 0 means never expire on the server, so None maps to 0
                ttl_seconds: ttl_seconds.unwrap_or(0),
            })
            .await
            .map_err(|e| Error::from_status(e))?;
        Ok(())
    }

    /// Retrieve a value from the cache.
    ///
    /// Returns `None` if the key doesn't exist or has expired.
    ///
    /// # Arguments
    /// * `key` - The cache key to look up
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// if let Some(value) = client.get("my-key").await? {
    ///     println!("Found: {}", value);
    /// } else {
    ///     println!("Key not found");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get(&self, key: impl Into<String>) -> Result<Option<String>, Error> {
        let mut client = self.inner.clone();
        let response = client
            .get(GetRequest { key: key.into() })
            .await
            .map_err(Error::from)?;

        let inner = response.into_inner();
        // Handle optional field - None means key not found
        match inner.value {
            Some(v) if !v.is_empty() => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Delete a key from the cache.
    ///
    /// Returns `true` if the key existed and was deleted, `false` otherwise.
    ///
    /// # Arguments
    /// * `key` - The cache key to delete
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// let was_deleted = client.delete("my-key").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete(&self, key: impl Into<String>) -> Result<bool, Error> {
        let mut client = self.inner.clone();
        let response = client
            .delete(DeleteRequest { key: key.into() })
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().deleted)
    }

    /// Delete all entries from the cache.
    ///
    /// Returns the approximate number of entries that were deleted.
    ///
    /// # ⚠️ Warning: Destructive Operation
    ///
    /// This operation:
    /// - **Immediately deletes ALL entries** from the cache
    /// - **Cannot be undone** - all data is permanently lost
    /// - **Affects all clients/services** using the same cache instance
    /// - Should be used with **extreme caution** in production environments
    ///
    /// # Note
    ///
    /// The returned count may be slightly inaccurate in concurrent scenarios
    /// where other operations occur simultaneously. This is acceptable for a cache
    /// where the count is informational.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// let deleted_count = client.delete_all().await?;
    /// println!("Deleted {} entries", deleted_count);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_all(&self) -> Result<u64, Error> {
        let mut client = self.inner.clone();
        let response = client
            .delete_all(proto::DeleteAllRequest {})
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().deleted_count)
    }

    /// Check if a key exists in the cache.
    ///
    /// # Arguments
    /// * `key` - The cache key to check
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// if client.contains("my-key").await? {
    ///     println!("Key exists!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn contains(&self, key: impl Into<String>) -> Result<bool, Error> {
        let mut client = self.inner.clone();
        let response = client
            .contains(ContainsRequest { key: key.into() })
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().exists)
    }

    /// List all keys matching a pattern.
    ///
    /// # Arguments
    /// * `pattern` - Optional glob pattern (e.g., "user:*"). If None, returns all keys.
    /// * `limit` - Optional maximum number of keys to return. If None, uses server default.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// // Get all keys (up to server default limit)
    /// let all_keys = client.keys(None, None).await?;
    ///
    /// // Get first 100 keys
    /// let limited_keys = client.keys(None, Some(100)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn keys(&self, pattern: Option<&str>, limit: Option<u32>) -> Result<Vec<String>, Error> {
        let mut client = self.inner.clone();
        let response = client
            .keys(KeysRequest {
                pattern: pattern.map(|s| s.to_string()),
                limit: limit.unwrap_or(0),
            })
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().keys)
    }

    /// Search for keys matching a prefix with pagination support.
    ///
    /// Returns matching keys sorted alphabetically, along with the total count
    /// of all matching keys (before pagination).
    ///
    /// # Arguments
    /// * `prefix` - The prefix to match keys against (empty string matches all keys, max: 256 bytes)
    /// * `limit` - Maximum number of keys to return (default: 50, max: 250)
    /// * `skip` - Number of matching keys to skip for pagination (default: 0)
    ///
    /// # Returns
    /// A tuple of (matching_keys, total_count) where:
    /// - `matching_keys` - Keys matching the prefix, sorted alphabetically
    /// - `total_count` - Total number of keys matching the prefix (before skip/limit)
    ///
    /// # Performance Warning
    ///
    /// **This operation iterates through ALL entries on the server** to find matches.
    /// It is intended for querying and debugging, not for high-frequency operations.
    ///
    /// For large datasets, this can be memory and CPU intensive. Use with caution
    /// on stores with many entries. Consider using reasonable limits and avoid
    /// calling this in tight loops or performance-critical paths.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// // Find all keys starting with "user:"
    /// let (keys, total) = client.search_keys("user:", None, None).await?;
    /// println!("Found {} total matches, returning {}", total, keys.len());
    ///
    /// // Paginate through results (10 per page)
    /// let (page1, _) = client.search_keys("user:", Some(10), Some(0)).await?;  // First 10
    /// let (page2, _) = client.search_keys("user:", Some(10), Some(10)).await?; // Next 10
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search_keys(
        &self,
        prefix: &str,
        limit: Option<u32>,
        skip: Option<u32>,
    ) -> Result<(Vec<String>, u64), Error> {
        let mut client = self.inner.clone();
        let response = client
            .search_keys(SearchKeysRequest {
                prefix: prefix.to_string(),
                limit: limit.unwrap_or(0),
                skip: skip.unwrap_or(0),
            })
            .await
            .map_err(Error::from)?;
        let inner = response.into_inner();
        Ok((inner.keys, inner.total_count))
    }
}

// JSON extension methods (only available with "json" feature)
#[cfg(feature = "json")]
impl MemorizeClient {
    /// Store a JSON-serializable value in the cache.
    ///
    /// # Arguments
    /// * `key` - The cache key
    /// * `value` - The value to serialize and store
    /// * `ttl_seconds` - Optional time-to-live in seconds
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # use serde::{Serialize, Deserialize};
    /// # #[derive(Serialize)]
    /// # struct User { name: String }
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// let user = User { name: "Alice".into() };
    /// client.set_json("user:1", &user, Some(3600)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_json<T: serde::Serialize>(
        &self,
        key: impl Into<String>,
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> Result<(), Error> {
        let json = serde_json::to_string(value).map_err(Error::Serialization)?;
        self.set(key, json, ttl_seconds).await
    }

    /// Retrieve and deserialize a JSON value from the cache.
    ///
    /// Returns `None` if the key doesn't exist.
    ///
    /// # Arguments
    /// * `key` - The cache key to look up
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # use serde::{Serialize, Deserialize};
    /// # #[derive(Deserialize)]
    /// # struct User { name: String }
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// let user: Option<User> = client.get_json("user:1").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        key: impl Into<String>,
    ) -> Result<Option<T>, Error> {
        match self.get(key).await? {
            Some(json) => {
                let value = serde_json::from_str(&json).map_err(Error::Deserialization)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }
}
