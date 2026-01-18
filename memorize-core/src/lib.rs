//! # Memorize Core
//!
//! A simple in-memory key-value store with TTL (time-to-live) support.
//!
//! ## Features
//!
//! - Thread-safe storage using `DashMap` (lock-free concurrent access)
//! - Automatic expiration on read (lazy cleanup)
//! - Background cleanup task for each store instance
//! - All data stored as strings
//!
//! ## TTL Semantics
//!
//! When setting a value, the TTL (time-to-live) parameter controls expiration:
//!
//! - **TTL = 0**: The entry **never expires**. Internally implemented as ~100 years.
//! - **TTL > 0**: The entry expires after the specified number of seconds.
//!
//! This design avoids `Option<Instant>` complexity while being effectively infinite
//! for practical purposes.
//!
//! ## Tokio Runtime Requirement
//!
//! This library requires a Tokio runtime to be active when creating a `Store`,
//! as it spawns a background task for periodic cleanup of expired entries.
//! If no runtime is available, construction will panic with a descriptive error.
//!
//! ## Example
//!
//! ```rust,no_run
//! use memorize_core::{Store, StoreConfig};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create store with default config (60 second cleanup interval)
//!     let store = Store::new();
//!
//!     // Or with custom cleanup interval
//!     let config = StoreConfig::default()
//!         .with_cleanup_interval(Duration::from_secs(30));
//!     let store = Store::with_config(config);
//!
//!     // Store a value with 60 second TTL
//!     store.set("user:123", "John Doe", 60).unwrap();
//!
//!     // Store a value that never expires (TTL = 0)
//!     store.set("config:setting", "value", 0).unwrap();
//!
//!     // Retrieve the value
//!     if let Some(value) = store.get("user:123") {
//!         println!("User: {}", value);
//!     }
//!
//!     // Delete a key
//!     store.delete("user:123");
//!
//!     // Manual cleanup (also done automatically by background task)
//!     let removed_count = store.cleanup();
//! }
//! ```

mod config;
mod entry;
mod store;

pub use config::StoreConfig;
pub use entry::Entry;
pub use store::{SetError, Store};

// Re-export search constants for use by server layer
pub use store::DEFAULT_SEARCH_LIMIT;
pub use store::MAX_SEARCH_LIMIT;
