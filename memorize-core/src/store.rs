use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::config::StoreConfig;
use crate::entry::Entry;

/// Error type for set operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    /// Storage capacity exceeded
    StorageFull {
        /// Current storage usage in bytes
        current_bytes: usize,
        /// Maximum storage capacity in bytes
        max_bytes: usize,
    },
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetError::StorageFull { current_bytes, max_bytes } => {
                write!(
                    f,
                    "Storage full: {} bytes used of {} bytes maximum",
                    current_bytes, max_bytes
                )
            }
        }
    }
}

impl std::error::Error for SetError {}

/// Internal shared state for the store
struct StoreInner {
    data: DashMap<String, Entry>,
    /// Sender to signal shutdown to the cleanup task
    shutdown_tx: watch::Sender<bool>,
    /// Current total size of stored data in bytes (keys + values)
    current_size: AtomicUsize,
    /// Maximum storage size in bytes (0 = unlimited)
    max_storage_bytes: usize,
}

/// Thread-safe in-memory key-value store with TTL support
/// 
/// Uses `DashMap` for lock-free concurrent access. Reads never block other reads,
/// and writes only block access to the specific key being written.
/// 
/// Each store spawns its own background cleanup task that periodically removes
/// expired entries. The cleanup task is automatically stopped when the store is dropped.
/// 
/// # Storage Limits
/// 
/// The store has a configurable maximum storage size (default: 100MB).
/// When full, new inserts will be rejected with `SetError::StorageFull`.
/// 
/// # Example
/// 
/// ```rust,no_run
/// use memorize_core::{Store, StoreConfig};
/// use std::time::Duration;
/// 
/// #[tokio::main]
/// async fn main() {
///     // Create store with default config (60s cleanup, 100MB max)
///     let store = Store::new();
///     
///     // Or with custom config
///     let config = StoreConfig::default()
///         .with_cleanup_interval(Duration::from_secs(30))
///         .with_max_storage_mb(256);
///     let store = Store::with_config(config);
///     
///     store.set("key", "value", 300).unwrap(); // 5 minute TTL
/// }
/// ```
/// Default limit for search_keys operations
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// Maximum limit for search_keys operations (hard cap)
pub const MAX_SEARCH_LIMIT: usize = 250;

/// Maximum number of keys to scan during search operations.
/// Prevents unbounded memory allocation from a single search request.
pub const MAX_SEARCH_SCAN: usize = 10_000;

#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

impl Store {
    /// Creates a new store with default configuration
    /// 
    /// # Panics
    /// 
    /// Panics if called outside of a Tokio runtime context. The store requires
    /// a runtime to spawn its background cleanup task.
    pub fn new() -> Self {
        Self::with_config(StoreConfig::default())
    }

    /// Creates a new store with custom configuration
    /// 
    /// # Panics
    /// 
    /// Panics if called outside of a Tokio runtime context. The store requires
    /// a runtime to spawn its background cleanup task.
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// use memorize_core::{Store, StoreConfig};
    /// use std::time::Duration;
    /// 
    /// #[tokio::main]
    /// async fn main() {
    ///     let config = StoreConfig::default()
    ///         .with_cleanup_interval(Duration::from_secs(30));
    ///     let store = Store::with_config(config);
    /// }
    /// ```
    pub fn with_config(config: StoreConfig) -> Self {
        // Verify that a Tokio runtime is available before proceeding.
        // This provides a clear error message instead of a cryptic panic from tokio::spawn.
        if tokio::runtime::Handle::try_current().is_err() {
            panic!(
                "memorize_core::Store requires a Tokio runtime. \
                 Ensure you are calling Store::new() or Store::with_config() \
                 from within a #[tokio::main] or #[tokio::test] context, \
                 or from code running on a Tokio runtime."
            );
        }
        
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        
        let inner = Arc::new(StoreInner {
            data: DashMap::new(),
            shutdown_tx,
            current_size: AtomicUsize::new(0),
            max_storage_bytes: config.max_storage_bytes,
        });

        // Spawn the background cleanup task
        let cleanup_inner = Arc::clone(&inner);
        tokio::spawn(Self::cleanup_task(cleanup_inner, config.cleanup_interval, shutdown_rx));

        Self { inner }
    }

    /// Background task that periodically cleans up expired entries
    async fn cleanup_task(
        inner: Arc<StoreInner>,
        interval: Duration,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick - we want to wait for the interval first
        ticker.tick().await;
        
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    Self::cleanup_internal(&inner);
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        // Shutdown signal received
                        break;
                    }
                }
            }
        }
    }

    /// Calculate the size of a key-value pair in bytes
    fn entry_size(key: &str, value: &str) -> usize {
        key.len() + value.len()
    }

    /// Internal cleanup logic (shared between manual and background cleanup)
    fn cleanup_internal(inner: &StoreInner) -> usize {
        let mut removed_count = 0;
        let mut removed_bytes = 0usize;
        
        inner.data.retain(|key, entry| {
            if entry.is_expired() {
                removed_bytes += Self::entry_size(key, entry.value());
                removed_count += 1;
                false
            } else {
                true
            }
        });
        
        if removed_bytes > 0 {
            inner.current_size.fetch_sub(removed_bytes, Ordering::Relaxed);
        }
        
        removed_count
    }

    /// Stores a value with the given key and TTL (time-to-live) in seconds
    /// 
    /// If the key already exists, the value is overwritten.
    /// 
    /// # TTL Semantics
    /// 
    /// - **TTL of 0** means the entry **never expires**. Internally this is implemented
    ///   as an expiration time approximately 100 years in the future. This avoids the
    ///   complexity of `Option<Instant>` while being effectively infinite for practical
    ///   purposes. The entry will still be subject to manual deletion or server restart.
    /// 
    /// - **Non-zero TTL** is the time-to-live in seconds. Values are capped at ~100 years
    ///   to prevent arithmetic overflow when calculating expiration times.
    /// 
    /// # Errors
    /// 
    /// Returns `SetError::StorageFull` if the storage limit would be exceeded.
    /// 
    /// # Note on Storage Limits
    /// 
    /// The storage limit is enforced as a "soft limit" for performance reasons.
    /// In highly concurrent scenarios, the actual storage may briefly exceed the
    /// configured limit due to the race between checking the limit and inserting
    /// the entry. This trade-off avoids holding locks during the entire set operation,
    /// which would significantly impact throughput.
    pub fn set(&self, key: impl Into<String>, value: impl Into<String>, ttl_seconds: u64) -> Result<(), SetError> {
        let key = key.into();
        let value = value.into();
        let new_entry_size = Self::entry_size(&key, &value);
        
        // Check storage limit (0 = unlimited)
        // Note: This is a soft limit check - see method documentation for details
        if self.inner.max_storage_bytes > 0 {
            // Check if we're updating an existing key (need to account for size difference)
            let old_entry_size = self.inner.data
                .get(&key)
                .map(|entry| Self::entry_size(&key, entry.value().value()))
                .unwrap_or(0);
            
            let size_delta = new_entry_size as isize - old_entry_size as isize;
            
            if size_delta > 0 {
                let current = self.inner.current_size.load(Ordering::Relaxed);
                let new_size = current.saturating_add(size_delta as usize);
                
                if new_size > self.inner.max_storage_bytes {
                    return Err(SetError::StorageFull {
                        current_bytes: current,
                        max_bytes: self.inner.max_storage_bytes,
                    });
                }
            }
        }
        
        // Cap TTL to ~100 years to prevent overflow when adding to Instant.
        // This value is used both as a maximum for explicit TTLs and as the
        // "never expire" duration when TTL is 0.
        const MAX_TTL_SECONDS: u64 = 100 * 365 * 24 * 60 * 60; // ~100 years
        
        // TTL of 0 means "never expire" - we implement this as expiring in ~100 years
        // rather than using Option<Instant> to keep the data structure simple and
        // avoid branching in hot paths. For all practical purposes, this is infinite.
        let safe_ttl = if ttl_seconds == 0 {
            MAX_TTL_SECONDS
        } else {
            ttl_seconds.min(MAX_TTL_SECONDS)
        };
        
        let expires_at = Instant::now() + Duration::from_secs(safe_ttl);
        let entry = Entry::new(value, expires_at);
        
        // Insert and track size change
        // Note: Size tracking happens after insert, creating a brief window of inaccuracy.
        // This is acceptable as the limit is a soft limit (see method docs).
        let old_entry = self.inner.data.insert(key.clone(), entry);
        
        // Calculate actual size change based on what was actually replaced
        let actual_old_size = old_entry
            .map(|e| Self::entry_size(&key, e.value()))
            .unwrap_or(0);
        let size_delta = new_entry_size as isize - actual_old_size as isize;
        
        if size_delta > 0 {
            self.inner.current_size.fetch_add(size_delta as usize, Ordering::Relaxed);
        } else if size_delta < 0 {
            self.inner.current_size.fetch_sub((-size_delta) as usize, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Stores a value that expires immediately (for testing purposes)
    #[cfg(test)]
    fn set_expired(&self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        let entry_size = Self::entry_size(&key, &value);
        
        // Set expiration to a time in the past
        let expires_at = Instant::now() - Duration::from_secs(1);
        let entry = Entry::new(value, expires_at);
        self.inner.data.insert(key, entry);
        self.inner.current_size.fetch_add(entry_size, Ordering::Relaxed);
    }

    /// Retrieves a value by key
    /// 
    /// Returns `None` if the key doesn't exist or has expired.
    /// Expired entries are automatically removed.
    pub fn get(&self, key: &str) -> Option<String> {
        // Try to get the entry
        let entry = self.inner.data.get(key)?;

        if entry.value().is_expired() {
            // Drop the read reference before removing
            drop(entry);
            // Use remove_if to atomically verify expiration and remove.
            // This prevents a race where another thread replaces the entry
            // between our check and removal, which would cause size drift.
            if let Some((k, removed)) = self.inner.data.remove_if(key, |_, v| v.is_expired()) {
                let entry_size = Self::entry_size(&k, removed.value());
                self.inner.current_size.fetch_sub(entry_size, Ordering::Relaxed);
            }
            return None;
        }

        Some(entry.value().value().to_string())
    }

    /// Deletes a key from the store
    /// 
    /// Returns `true` if the key existed (regardless of expiration), `false` otherwise.
    #[must_use = "returns whether the key existed"]
    pub fn delete(&self, key: &str) -> bool {
        if let Some((k, entry)) = self.inner.data.remove(key) {
            let entry_size = Self::entry_size(&k, entry.value());
            self.inner.current_size.fetch_sub(entry_size, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Deletes all entries from the store.
    /// 
    /// Returns the approximate number of entries that were removed.
    /// 
    /// # ⚠️ Warning: Destructive Operation
    /// 
    /// This operation:
    /// - **Immediately removes ALL entries** from the store
    /// - **Cannot be undone** - all data is permanently lost
    /// - **Affects all concurrent readers/writers** - they will see an empty store
    /// - Should be used with **extreme caution** in production environments
    /// 
    /// Consider using API key authentication to protect this operation in production.
    /// 
    /// # Note
    /// 
    /// The returned count may be slightly inaccurate in concurrent scenarios
    /// where other threads add or remove entries between the count and clear operations.
    /// This is acceptable for a cache where the count is informational.
    pub fn delete_all(&self) -> usize {
        let count = self.inner.data.len();
        self.inner.data.clear();
        self.inner.current_size.store(0, Ordering::Relaxed);
        count
    }

    /// Manually triggers cleanup of all expired entries
    /// 
    /// Returns the number of entries removed.
    /// 
    /// Note: This is also done automatically by the background task.
    pub fn cleanup(&self) -> usize {
        Self::cleanup_internal(&self.inner)
    }

    /// Returns the number of entries in the store (including expired ones)
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.data.len()
    }

    /// Returns `true` if the store is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.data.is_empty()
    }

    /// Returns the current storage size in bytes
    pub fn size_bytes(&self) -> usize {
        self.inner.current_size.load(Ordering::Relaxed)
    }

    /// Returns the maximum storage size in bytes (0 = unlimited)
    pub fn max_size_bytes(&self) -> usize {
        self.inner.max_storage_bytes
    }

    /// Checks if a key exists and is not expired.
    /// 
    /// Expired entries are lazily removed when checked.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        match self.inner.data.get(key) {
            Some(entry) => {
                if entry.value().is_expired() {
                    // Drop the read reference before removing
                    drop(entry);
                    // Use remove_if to atomically verify expiration and remove.
                    // This prevents a race where another thread replaces the entry
                    // between our check and removal, which would cause size drift.
                    if let Some((k, removed)) = self.inner.data.remove_if(key, |_, v| v.is_expired()) {
                        let entry_size = Self::entry_size(&k, removed.value());
                        self.inner.current_size.fetch_sub(entry_size, Ordering::Relaxed);
                    }
                    false
                } else {
                    true
                }
            }
            None => false,
        }
    }

    /// Returns all keys that are not expired
    pub fn keys(&self) -> Vec<String> {
        self.inner.data
            .iter()
            .filter(|entry| !entry.value().is_expired())
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Returns keys that are not expired, with an optional limit
    /// 
    /// # Arguments
    /// * `limit` - Maximum number of keys to return. None means no limit.
    pub fn keys_with_limit(&self, limit: Option<usize>) -> Vec<String> {
        let iter = self.inner.data
            .iter()
            .filter(|entry| !entry.value().is_expired())
            .map(|entry| entry.key().clone());
        
        match limit {
            Some(n) => iter.take(n).collect(),
            None => iter.collect(),
        }
    }

    /// Searches for keys matching a prefix with pagination support.
    /// 
    /// Returns matching keys sorted alphabetically, along with the total count
    /// of all matching keys (before pagination).
    /// 
    /// # Arguments
    /// * `prefix` - The prefix to match keys against (empty string matches all keys)
    /// * `limit` - Maximum number of keys to return (default: 50, max: 250)
    /// * `skip` - Number of matching keys to skip for pagination (default: 0)
    /// 
    /// # Returns
    /// A tuple of (matching_keys, total_count) where:
    /// - `matching_keys` - Keys matching the prefix, sorted alphabetically, with skip/limit applied
    /// - `total_count` - Total number of keys matching the prefix (before skip/limit)
    /// 
    /// # Performance Warning
    /// 
    /// **This operation iterates through ALL entries in the store** to find matches.
    /// It is intended for querying and debugging, not for high-frequency operations.
    /// 
    /// For large datasets, this can be memory and CPU intensive because it:
    /// - Scans every key in the store
    /// - Collects all matching keys into memory
    /// - Sorts the results alphabetically
    /// 
    /// Use with caution on stores with many entries. Consider using reasonable
    /// limits and avoid calling this in tight loops or performance-critical paths.
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// # use memorize_core::Store;
    /// # let store = Store::new();
    /// // Find all keys starting with "user:"
    /// let (keys, total) = store.search_keys("user:", None, None);
    /// println!("Found {} total matches, returning first {}", total, keys.len());
    /// 
    /// // Paginate through results
    /// let (page1, _) = store.search_keys("user:", Some(10), Some(0));  // First 10
    /// let (page2, _) = store.search_keys("user:", Some(10), Some(10)); // Next 10
    /// ```
    pub fn search_keys(&self, prefix: &str, limit: Option<usize>, skip: Option<usize>) -> (Vec<String>, usize) {
        let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT);
        let skip = skip.unwrap_or(0);
        
        // Collect matching, non-expired keys with a scan cap to bound memory allocation.
        // Filter by prefix first to avoid cloning keys that don't match,
        // then check expiration (which may be more expensive).
        // The cap prevents a single request from causing OOM on large stores.
        let mut matching_keys: Vec<String> = self.inner.data
            .iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .filter(|entry| !entry.value().is_expired())
            .take(MAX_SEARCH_SCAN)
            .map(|entry| entry.key().clone())
            .collect();
        
        // Sort alphabetically for deterministic pagination
        matching_keys.sort();
        
        let total_count = matching_keys.len();
        
        // Apply skip and limit
        let result: Vec<String> = matching_keys
            .into_iter()
            .skip(skip)
            .take(limit)
            .collect();
        
        (result, total_count)
    }

    /// Gracefully shuts down the background cleanup task
    /// 
    /// This is called automatically when the store is dropped,
    /// but can be called manually if needed.
    pub fn shutdown(&self) {
        let _ = self.inner.shutdown_tx.send(true);
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StoreInner {
    fn drop(&mut self) {
        // Signal the cleanup task to stop when the store is dropped
        let _ = self.shutdown_tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Helper to create a store within a tokio runtime for tests
    fn create_test_store() -> Store {
        create_test_store_with_config(StoreConfig::default())
    }

    fn create_test_store_with_config(config: StoreConfig) -> Store {
        // Create a runtime for the background task
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        
        // Keep the runtime alive by leaking it (fine for tests)
        let rt = Box::leak(Box::new(rt));
        let _guard = rt.enter();
        
        Store::with_config(config)
    }

    #[test]
    fn test_set_and_get() {
        let store = create_test_store();
        store.set("key1", "value1", 60).unwrap();
        
        assert_eq!(store.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let store = create_test_store();
        assert_eq!(store.get("nonexistent"), None);
    }

    #[test]
    fn test_overwrite_key() {
        let store = create_test_store();
        store.set("key1", "value1", 60).unwrap();
        store.set("key1", "value2", 60).unwrap();
        
        assert_eq!(store.get("key1"), Some("value2".to_string()));
    }

    #[test]
    fn test_delete() {
        let store = create_test_store();
        store.set("key1", "value1", 60).unwrap();
        
        assert!(store.delete("key1"));
        assert_eq!(store.get("key1"), None);
        assert!(!store.delete("key1")); // Already deleted
    }

    #[test]
    fn test_delete_all() {
        let store = create_test_store();
        
        // Add some entries
        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        store.set("key3", "value3", 60).unwrap();
        
        assert_eq!(store.len(), 3);
        assert!(store.size_bytes() > 0);
        
        // Delete all
        let deleted = store.delete_all();
        assert_eq!(deleted, 3);
        
        // Verify everything is gone
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert_eq!(store.size_bytes(), 0);
        assert_eq!(store.get("key1"), None);
        assert_eq!(store.get("key2"), None);
        assert_eq!(store.get("key3"), None);
    }

    #[test]
    fn test_delete_all_empty_store() {
        let store = create_test_store();
        
        // Delete all on empty store should return 0
        let deleted = store.delete_all();
        assert_eq!(deleted, 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_delete_all_resets_size_tracking() {
        let config = StoreConfig::default().with_max_storage_bytes(100);
        let store = create_test_store_with_config(config);
        
        // Fill up storage
        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        
        let size_before = store.size_bytes();
        assert!(size_before > 0);
        
        // Delete all should reset size to 0
        store.delete_all();
        assert_eq!(store.size_bytes(), 0);
        
        // Should be able to add entries again (storage limit reset)
        store.set("new1", "value1", 60).unwrap();
        assert!(store.size_bytes() > 0);
    }

    #[test]
    fn test_expired_entry_returns_none() {
        let store = create_test_store();
        store.set_expired("key1", "value1"); // Expires immediately
        
        // Small sleep to ensure expiration
        thread::sleep(Duration::from_millis(10));
        
        assert_eq!(store.get("key1"), None);
    }

    #[test]
    fn test_cleanup() {
        // Use a long cleanup interval to prevent background task from interfering
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_secs(3600)); // 1 hour
        let store = create_test_store_with_config(config);
        
        store.set_expired("expired1", "value1");
        store.set_expired("expired2", "value2");
        store.set("valid", "value3", 60).unwrap();
        
        thread::sleep(Duration::from_millis(10));
        
        let removed = store.cleanup();
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("valid"), Some("value3".to_string()));
    }

    #[test]
    fn test_contains_key() {
        let store = create_test_store();
        store.set("key1", "value1", 60).unwrap();
        store.set_expired("expired", "value2");
        
        thread::sleep(Duration::from_millis(10));
        
        assert!(store.contains_key("key1"));
        assert!(!store.contains_key("expired"));
        assert!(!store.contains_key("nonexistent"));
    }

    #[test]
    fn test_keys() {
        let store = create_test_store();
        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        store.set_expired("expired", "value3");
        
        thread::sleep(Duration::from_millis(10));
        
        let mut keys = store.keys();
        keys.sort();
        
        assert_eq!(keys, vec!["key1", "key2"]);
    }

    #[test]
    fn test_len_and_is_empty() {
        let store = create_test_store();
        
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        
        store.set("key1", "value1", 60).unwrap();
        
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_extreme_ttl_does_not_panic() {
        let store = create_test_store();
        // This should not panic - TTL is capped internally
        store.set("key1", "value1", u64::MAX).unwrap();
        
        // Should still be retrievable (won't expire for ~100 years)
        assert_eq!(store.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn test_zero_ttl_means_never_expire() {
        let store = create_test_store();
        // TTL of 0 means the entry never expires
        store.set("key1", "value1", 0).unwrap();
        
        // Wait a bit to ensure it doesn't expire
        thread::sleep(Duration::from_millis(50));
        
        // Should still be retrievable
        assert_eq!(store.get("key1"), Some("value1".to_string()));
        assert!(store.contains_key("key1"));
    }

    #[test]
    fn test_concurrent_writes() {
        let store = Arc::new(create_test_store());
        let mut handles = vec![];
        
        // Spawn 10 threads, each writing 100 keys
        for thread_id in 0..10 {
            let store = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("thread{}:key{}", thread_id, i);
                    let value = format!("value{}", i);
                    store.set(key, value, 60).unwrap();
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Verify all 1000 keys were written
        assert_eq!(store.len(), 1000);
    }

    #[test]
    fn test_concurrent_reads_and_writes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        
        let store = Arc::new(create_test_store());
        
        // Pre-populate with some data
        for i in 0..100 {
            store.set(format!("key{}", i), format!("value{}", i), 60).unwrap();
        }
        
        let successful_reads = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];
        
        // Spawn reader threads
        for _ in 0..5 {
            let store = Arc::clone(&store);
            let successful_reads = Arc::clone(&successful_reads);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    if store.get(&format!("key{}", i)).is_some() {
                        successful_reads.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
            handles.push(handle);
        }
        
        // Spawn writer threads (writing to different keys)
        for thread_id in 0..5 {
            let store = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("new_thread{}:key{}", thread_id, i);
                    store.set(key, "new_value", 60).unwrap();
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // All reads should have succeeded (original 100 keys still exist)
        assert_eq!(successful_reads.load(Ordering::SeqCst), 500); // 5 threads * 100 reads
        
        // Should have original 100 + 500 new keys
        assert_eq!(store.len(), 600);
    }

    #[test]
    fn test_concurrent_writes_to_same_key() {
        let store = Arc::new(create_test_store());
        let mut handles = vec![];
        
        // Spawn 10 threads, all writing to the same key
        for thread_id in 0..10 {
            let store = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let value = format!("thread{}:iteration{}", thread_id, i);
                    store.set("contested_key", value, 60).unwrap();
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Should only have 1 key (all writes went to the same key)
        assert_eq!(store.len(), 1);
        
        // Should have some value (we don't know which thread won last)
        assert!(store.get("contested_key").is_some());
    }

    #[test]
    fn test_concurrent_cleanup_with_operations() {
        use std::thread::JoinHandle;
        
        let store = Arc::new(create_test_store());
        
        // Pre-populate with expiring and non-expiring data
        for i in 0..50 {
            store.set_expired(format!("expiring{}", i), "value"); // Expires immediately
            store.set(format!("persistent{}", i), "value", 60).unwrap();
        }
        
        thread::sleep(Duration::from_millis(10)); // Ensure expiration
        
        let mut handles: Vec<JoinHandle<()>> = vec![];
        
        // Spawn cleanup thread
        let store_cleanup = Arc::clone(&store);
        let cleanup_handle = thread::spawn(move || {
            let _ = store_cleanup.cleanup(); // Ignore return value to match () type
        });
        handles.push(cleanup_handle);
        
        // Spawn reader threads simultaneously
        for _ in 0..3 {
            let store = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for i in 0..50 {
                    // These should return None (expired) or be cleaned up
                    let _ = store.get(&format!("expiring{}", i));
                    // These should still exist
                    let _ = store.get(&format!("persistent{}", i));
                }
            });
            handles.push(handle);
        }
        
        // Spawn writer thread simultaneously
        let store_writer = Arc::clone(&store);
        let writer_handle = thread::spawn(move || {
            for i in 0..50 {
                store_writer.set(format!("new{}", i), "value", 60).unwrap();
            }
        });
        handles.push(writer_handle);
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Expiring keys should be gone, persistent + new should remain
        // persistent: 50, new: 50 = 100
        assert_eq!(store.len(), 100);
        
        // Verify persistent keys still exist
        for i in 0..50 {
            assert!(store.contains_key(&format!("persistent{}", i)));
            assert!(store.contains_key(&format!("new{}", i)));
        }
    }

    #[tokio::test]
    async fn test_background_cleanup_runs() {
        // Create store with very short cleanup interval
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_millis(50));
        let store = Store::with_config(config);
        
        // Add some entries that expire quickly
        store.set_expired("expire1", "value1");
        store.set_expired("expire2", "value2");
        store.set("keep", "value3", 60).unwrap();
        
        // Initially all 3 entries exist (even if expired)
        assert_eq!(store.len(), 3);
        
        // Wait for background cleanup to run (interval + some buffer)
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Background cleanup should have removed expired entries
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("keep"), Some("value3".to_string()));
    }

    #[tokio::test]
    async fn test_store_clone_shares_data() {
        let store1 = Store::new();
        let store2 = store1.clone();
        
        store1.set("key1", "value1", 60).unwrap();
        
        // Both stores should see the same data
        assert_eq!(store2.get("key1"), Some("value1".to_string()));
        
        store2.set("key2", "value2", 60).unwrap();
        assert_eq!(store1.get("key2"), Some("value2".to_string()));
    }

    #[tokio::test]
    async fn test_shutdown_stops_cleanup_task() {
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_millis(10));
        let store = Store::with_config(config);
        
        // TTL=0 means never expire
        store.set("key1", "value1", 0).unwrap();
        
        // Explicitly shutdown
        store.shutdown();
        
        // Give some time for shutdown to process
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // The entry should still be there (TTL=0 means never expire)
        assert_eq!(store.get("key1"), Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_stores_independent_cleanup() {
        let config1 = StoreConfig::default()
            .with_cleanup_interval(Duration::from_millis(50));
        let config2 = StoreConfig::default()
            .with_cleanup_interval(Duration::from_secs(60)); // Long interval
        
        let store1 = Store::with_config(config1);
        let store2 = Store::with_config(config2);
        
        // Add expiring entries to both
        store1.set_expired("expire", "value");
        store2.set("keep", "value", 60).unwrap(); // Non-expiring entry
        
        // Wait for store1's cleanup to run
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // store1 should be cleaned up
        assert_eq!(store1.len(), 0);
        
        // store2 should still have its entry (independent store)
        assert_eq!(store2.len(), 1);
        assert_eq!(store2.get("keep"), Some("value".to_string()));
    }

    #[test]
    fn test_storage_size_tracking() {
        let config = StoreConfig::default().with_max_storage_mb(0); // Unlimited
        let store = create_test_store_with_config(config);
        
        assert_eq!(store.size_bytes(), 0);
        
        store.set("key1", "value1", 60).unwrap(); // 4 + 6 = 10 bytes
        assert_eq!(store.size_bytes(), 10);
        
        store.set("key2", "value2", 60).unwrap(); // 4 + 6 = 10 bytes
        assert_eq!(store.size_bytes(), 20);
        
        // Overwrite with smaller value
        store.set("key1", "x", 60).unwrap(); // 4 + 1 = 5 bytes (delta: -5)
        assert_eq!(store.size_bytes(), 15);
        
        // Delete
        let _ = store.delete("key1");
        assert_eq!(store.size_bytes(), 10);
        
        let _ = store.delete("key2");
        assert_eq!(store.size_bytes(), 0);
    }

    #[test]
    fn test_storage_limit_enforced() {
        // 50 bytes max
        let config = StoreConfig::default().with_max_storage_bytes(50);
        let store = create_test_store_with_config(config);
        
        // Each entry is 10 bytes (4 key + 6 value)
        store.set("key1", "value1", 60).unwrap(); // 10 bytes - OK
        store.set("key2", "value2", 60).unwrap(); // 20 bytes - OK
        store.set("key3", "value3", 60).unwrap(); // 30 bytes - OK
        store.set("key4", "value4", 60).unwrap(); // 40 bytes - OK
        store.set("key5", "value5", 60).unwrap(); // 50 bytes - OK (exactly at limit)
        
        // This should fail - would exceed 50 bytes
        let result = store.set("key6", "value6", 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));
        
        // Verify the failed insert didn't add the key
        assert!(!store.contains_key("key6"));
        assert_eq!(store.len(), 5);
        assert_eq!(store.size_bytes(), 50);
    }

    #[test]
    fn test_storage_limit_allows_overwrites() {
        // 30 bytes max
        let config = StoreConfig::default().with_max_storage_bytes(30);
        let store = create_test_store_with_config(config);
        
        store.set("key1", "value1", 60).unwrap(); // 10 bytes
        store.set("key2", "value2", 60).unwrap(); // 20 bytes
        store.set("key3", "value3", 60).unwrap(); // 30 bytes - at limit
        
        // Overwriting an existing key with same size should work
        store.set("key1", "valueX", 60).unwrap();
        
        // Overwriting with smaller value should work
        store.set("key1", "x", 60).unwrap(); // Now 5 bytes for key1
        
        // Now we have room for more
        assert_eq!(store.size_bytes(), 25);
        
        // Can add 5 more bytes
        store.set("k", "1234", 60).unwrap(); // 1 + 4 = 5 bytes
        assert_eq!(store.size_bytes(), 30);
    }

    #[test]
    fn test_storage_freed_on_expiry_cleanup() {
        let config = StoreConfig::default()
            .with_max_storage_bytes(100)
            .with_cleanup_interval(Duration::from_secs(3600)); // Long interval
        let store = create_test_store_with_config(config);
        
        // Add some expiring entries
        store.set_expired("exp1", "value1"); // Will count towards size
        store.set_expired("exp2", "value2");
        store.set("keep", "value3", 60).unwrap();
        
        let size_before = store.size_bytes();
        assert!(size_before > 0);
        
        thread::sleep(Duration::from_millis(10));
        
        // Cleanup should free memory
        let removed = store.cleanup();
        assert_eq!(removed, 2);
        
        // Size should be reduced
        let size_after = store.size_bytes();
        assert!(size_after < size_before);
        assert_eq!(size_after, "keep".len() + "value3".len());
    }

    #[test]
    fn test_unlimited_storage_when_zero() {
        let config = StoreConfig::default().with_max_storage_mb(0);
        let store = create_test_store_with_config(config);
        
        assert_eq!(store.max_size_bytes(), 0);
        
        // Should be able to add many entries without hitting limit
        for i in 0..1000 {
            store.set(format!("key{}", i), format!("value{}", i), 60).unwrap();
        }
        
        assert_eq!(store.len(), 1000);
    }

    #[test]
    fn test_storage_full_error_contains_correct_info() {
        // Set a very small limit: 100 bytes
        let config = StoreConfig::default().with_max_storage_bytes(100);
        let store = create_test_store_with_config(config);
        
        // Fill up storage
        // key="k00" (3 bytes) + value="val00" (5 bytes) = 8 bytes each
        for i in 0..12 {
            store.set(format!("k{:02}", i), format!("val{:02}", i), 60).unwrap();
        }
        
        // Verify we're near capacity (12 * 8 = 96 bytes)
        let current_size = store.size_bytes();
        assert_eq!(current_size, 96, "Should be at 96 bytes");
        
        // Try to add more - should fail with StorageFull
        // "overflow" (8) + "data" (4) = 12 bytes, would put us at 108 > 100
        let result = store.set("overflow", "data", 60);
        
        match result {
            Err(SetError::StorageFull { current_bytes, max_bytes }) => {
                assert_eq!(max_bytes, 100, "max_bytes should be 100");
                assert_eq!(current_bytes, 96, "current_bytes should be 96");
            }
            Ok(_) => panic!("Expected StorageFull error, but set succeeded"),
        }
        
        // Verify the key wasn't added
        assert!(!store.contains_key("overflow"));
        assert_eq!(store.len(), 12);
    }

    #[test]
    fn test_storage_limit_with_mb_config() {
        // Test the with_max_storage_mb helper (1 MB = 1,048,576 bytes)
        let config = StoreConfig::default().with_max_storage_mb(1);
        let store = create_test_store_with_config(config);
        
        assert_eq!(store.max_size_bytes(), 1024 * 1024);
        
        // Create a large value (~100KB)
        let large_value = "x".repeat(100_000);
        
        // Should be able to add ~10 of these
        for i in 0..10 {
            store.set(format!("large{}", i), &large_value, 60).unwrap();
        }
        
        // Size should be approximately 10 * (6 + 100_000) = ~1,000,060 bytes
        let size = store.size_bytes();
        assert!(size > 1_000_000, "Should have stored ~1MB");
        assert!(size < 1_048_576, "Should still be under limit");
        
        // One more should fail
        let result = store.set("large10", &large_value, 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));
    }

    #[test]
    fn test_storage_limit_delete_frees_space_for_new_entries() {
        // 50 bytes limit
        let config = StoreConfig::default().with_max_storage_bytes(50);
        let store = create_test_store_with_config(config);
        
        // Fill it up: key="key0" (4) + value="val00" (5) = 9 bytes each
        // 5 entries * 9 bytes = 45 bytes
        for i in 0..5 {
            store.set(format!("key{}", i), format!("val{:02}", i), 60).unwrap();
        }
        assert_eq!(store.size_bytes(), 45);
        
        // Can't add more (would exceed 50)
        // "newkey" (6) + "value!" (6) = 12 bytes, 45 + 12 = 57 > 50
        let result = store.set("newkey", "value!", 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));
        
        // Delete one entry (frees 9 bytes)
        let _ = store.delete("key0");
        assert_eq!(store.size_bytes(), 36);
        
        // Now we can add a new entry (36 + 12 = 48 <= 50)
        store.set("newkey", "value!", 60).unwrap();
        assert_eq!(store.size_bytes(), 48);
        assert!(store.contains_key("newkey"));
    }

    // === search_keys tests ===

    #[test]
    fn test_search_keys_basic_prefix() {
        let store = create_test_store();
        
        store.set("user:1", "alice", 60).unwrap();
        store.set("user:2", "bob", 60).unwrap();
        store.set("user:3", "charlie", 60).unwrap();
        store.set("session:1", "data1", 60).unwrap();
        store.set("session:2", "data2", 60).unwrap();
        
        let (keys, total) = store.search_keys("user:", None, None);
        assert_eq!(total, 3);
        assert_eq!(keys, vec!["user:1", "user:2", "user:3"]);
        
        let (keys, total) = store.search_keys("session:", None, None);
        assert_eq!(total, 2);
        assert_eq!(keys, vec!["session:1", "session:2"]);
    }

    #[test]
    fn test_search_keys_empty_prefix_matches_all() {
        let store = create_test_store();
        
        store.set("a", "1", 60).unwrap();
        store.set("b", "2", 60).unwrap();
        store.set("c", "3", 60).unwrap();
        
        let (keys, total) = store.search_keys("", None, None);
        assert_eq!(total, 3);
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_search_keys_no_matches() {
        let store = create_test_store();
        
        store.set("user:1", "alice", 60).unwrap();
        store.set("user:2", "bob", 60).unwrap();
        
        let (keys, total) = store.search_keys("nonexistent:", None, None);
        assert_eq!(total, 0);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_search_keys_default_limit() {
        let store = create_test_store();
        
        // Add 100 keys
        for i in 0..100 {
            store.set(format!("key:{:03}", i), "value", 60).unwrap();
        }
        
        // Default limit is 50
        let (keys, total) = store.search_keys("key:", None, None);
        assert_eq!(total, 100);
        assert_eq!(keys.len(), 50);
        
        // Keys should be sorted, so first 50
        assert_eq!(keys[0], "key:000");
        assert_eq!(keys[49], "key:049");
    }

    #[test]
    fn test_search_keys_custom_limit() {
        let store = create_test_store();
        
        for i in 0..20 {
            store.set(format!("item:{:02}", i), "value", 60).unwrap();
        }
        
        let (keys, total) = store.search_keys("item:", Some(5), None);
        assert_eq!(total, 20);
        assert_eq!(keys.len(), 5);
        assert_eq!(keys, vec!["item:00", "item:01", "item:02", "item:03", "item:04"]);
    }

    #[test]
    fn test_search_keys_pagination_with_skip() {
        let store = create_test_store();
        
        for i in 0..10 {
            store.set(format!("p:{}", i), "value", 60).unwrap();
        }
        
        // First page
        let (page1, total) = store.search_keys("p:", Some(3), Some(0));
        assert_eq!(total, 10);
        assert_eq!(page1, vec!["p:0", "p:1", "p:2"]);
        
        // Second page
        let (page2, _) = store.search_keys("p:", Some(3), Some(3));
        assert_eq!(page2, vec!["p:3", "p:4", "p:5"]);
        
        // Third page
        let (page3, _) = store.search_keys("p:", Some(3), Some(6));
        assert_eq!(page3, vec!["p:6", "p:7", "p:8"]);
        
        // Last page (partial)
        let (page4, _) = store.search_keys("p:", Some(3), Some(9));
        assert_eq!(page4, vec!["p:9"]);
    }

    #[test]
    fn test_search_keys_skip_beyond_results() {
        let store = create_test_store();
        
        store.set("key:1", "value", 60).unwrap();
        store.set("key:2", "value", 60).unwrap();
        
        let (keys, total) = store.search_keys("key:", Some(10), Some(100));
        assert_eq!(total, 2);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_search_keys_sorted_alphabetically() {
        let store = create_test_store();
        
        // Insert in non-alphabetical order
        store.set("z:last", "value", 60).unwrap();
        store.set("a:first", "value", 60).unwrap();
        store.set("m:middle", "value", 60).unwrap();
        
        let (keys, _) = store.search_keys("", None, None);
        assert_eq!(keys, vec!["a:first", "m:middle", "z:last"]);
    }

    #[test]
    fn test_search_keys_excludes_expired() {
        let store = create_test_store();
        
        store.set("active:1", "value", 60).unwrap();
        store.set("active:2", "value", 60).unwrap();
        store.set_expired("active:expired", "value");
        
        thread::sleep(Duration::from_millis(10));
        
        let (keys, total) = store.search_keys("active:", None, None);
        assert_eq!(total, 2);
        assert_eq!(keys, vec!["active:1", "active:2"]);
    }

    #[test]
    fn test_search_keys_empty_store() {
        let store = create_test_store();
        
        let (keys, total) = store.search_keys("any:", None, None);
        assert_eq!(total, 0);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_search_keys_limit_larger_than_results() {
        let store = create_test_store();
        
        store.set("x:1", "value", 60).unwrap();
        store.set("x:2", "value", 60).unwrap();
        
        let (keys, total) = store.search_keys("x:", Some(100), None);
        assert_eq!(total, 2);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_search_keys_max_limit_enforced() {
        let store = create_test_store();
        
        // Add 300 keys (more than max limit of 250)
        for i in 0..300 {
            store.set(format!("max:{:03}", i), "value", 60).unwrap();
        }
        
        // Request 500 - should be capped to 250
        let (keys, total) = store.search_keys("max:", Some(500), None);
        assert_eq!(total, 300);
        assert_eq!(keys.len(), 250, "Limit should be capped to MAX_SEARCH_LIMIT (250)");
        
        // Verify the first and last keys are correct (sorted)
        assert_eq!(keys[0], "max:000");
        assert_eq!(keys[249], "max:249");
    }
}
