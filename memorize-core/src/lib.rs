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
