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

    /// Server storage is full - cannot store more data
    #[error("Storage full: {0}")]
    StorageFull(String),

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

impl Error {
    /// Returns `true` if this error indicates the server storage is full.
    pub fn is_storage_full(&self) -> bool {
        matches!(self, Error::StorageFull(_))
    }

    /// Creates an error from a tonic Status, converting RESOURCE_EXHAUSTED to StorageFull
    pub(crate) fn from_status(status: tonic::Status) -> Self {
        if status.code() == tonic::Code::ResourceExhausted {
            Error::StorageFull(status.message().to_string())
        } else {
            Error::Grpc(status)
        }
    }
}
