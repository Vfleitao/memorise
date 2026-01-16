//! Memorize gRPC Protocol Definitions
//!
//! This crate contains the generated gRPC code for the Memorize cache service.

/// Generated protobuf/gRPC code
pub mod memorize {
    tonic::include_proto!("memorize");
}

pub use memorize::*;
