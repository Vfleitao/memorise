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
//! ```rust,no_run
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

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

// Include the generated protobuf code
pub mod proto {
    tonic::include_proto!("memorize");
}

use proto::memorize_client::MemorizeClient as GrpcClient;
use proto::{ContainsRequest, DeleteRequest, GetRequest, KeysRequest, SetRequest};

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
/// The client is thread-safe and can be cloned cheaply (it uses internal Arc).
#[derive(Clone)]
pub struct MemorizeClient {
    inner: Arc<RwLock<InterceptedClient>>,
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

        Ok(Self {
            inner: Arc::new(RwLock::new(client)),
        })
    }

    /// Store a value in the cache.
    ///
    /// # Arguments
    /// * `key` - The cache key
    /// * `value` - The value to store
    /// * `ttl_seconds` - Optional time-to-live in seconds. If None, the entry never expires.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// // Store with 5-minute TTL
    /// client.set("key", "value", Some(300)).await?;
    ///
    /// // Store without expiration
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
        let mut client = self.inner.write().await;
        client
            .set(SetRequest {
                key: key.into(),
                value: value.into(),
                ttl_seconds: ttl_seconds.unwrap_or(0),
            })
            .await
            .map_err(Error::from)?;
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
        let mut client = self.inner.write().await;
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
        let mut client = self.inner.write().await;
        let response = client
            .delete(DeleteRequest { key: key.into() })
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().deleted)
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
        let mut client = self.inner.write().await;
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
    ///
    /// # Example
    /// ```rust,no_run
    /// # use memorize_client::MemorizeClient;
    /// # async fn example() -> Result<(), memorize_client::Error> {
    /// # let client = MemorizeClient::connect("http://localhost:50051").await?;
    /// // Get all keys
    /// let all_keys = client.keys(None).await?;
    ///
    /// // Get keys matching pattern
    /// let user_keys = client.keys(Some("user:*")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn keys(&self, pattern: Option<&str>) -> Result<Vec<String>, Error> {
        let mut client = self.inner.write().await;
        let response = client
            .keys(KeysRequest {
                pattern: pattern.map(|s| s.to_string()),
            })
            .await
            .map_err(Error::from)?;
        Ok(response.into_inner().keys)
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
