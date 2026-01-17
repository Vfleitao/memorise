//! Error types for the Memorize client.

use thiserror::Error;

/// Errors that can occur when using the Memorize client.
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to connect to the server
    #[error("Connection error: {0}")]
    Connection(String),

    /// gRPC transport error
    #[error("Transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status error (e.g., authentication failure, not found)
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),

    /// JSON serialization error (requires `json` feature)
    #[cfg(feature = "json")]
    #[error("Serialization error: {0}")]
    Serialization(#[source] serde_json::Error),

    /// JSON deserialization error (requires `json` feature)
    #[cfg(feature = "json")]
    #[error("Deserialization error: {0}")]
    Deserialization(#[source] serde_json::Error),
}
