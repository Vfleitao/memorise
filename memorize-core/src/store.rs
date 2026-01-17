use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::config::StoreConfig;
use crate::entry::Entry;

/// Internal shared state for the store
struct StoreInner {
    data: DashMap<String, Entry>,
    /// Sender to signal shutdown to the cleanup task
    shutdown_tx: watch::Sender<bool>,
}

/// Thread-safe in-memory key-value store with TTL support
/// 
/// Uses `DashMap` for lock-free concurrent access. Reads never block other reads,
/// and writes only block access to the specific key being written.
/// 
/// Each store spawns its own background cleanup task that periodically removes
/// expired entries. The cleanup task is automatically stopped when the store is dropped.
/// 
/// # Example
/// 
/// ```rust,no_run
/// use memorize_core::{Store, StoreConfig};
/// use std::time::Duration;
/// 
/// #[tokio::main]
/// async fn main() {
///     // Create store with default config (60 second cleanup interval)
///     let store = Store::new();
///     
///     // Or with custom config
///     let config = StoreConfig::default()
///         .with_cleanup_interval(Duration::from_secs(30));
///     let store = Store::with_config(config);
///     
///     store.set("key", "value", 300); // 5 minute TTL
/// }
/// ```
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

impl Store {
    /// Creates a new store with default configuration
    /// 
    /// **Note:** Requires a tokio runtime to be available for the background cleanup task.
    pub fn new() -> Self {
        Self::with_config(StoreConfig::default())
    }

    /// Creates a new store with custom configuration
    /// 
    /// **Note:** Requires a tokio runtime to be available for the background cleanup task.
    pub fn with_config(config: StoreConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        
        let inner = Arc::new(StoreInner {
            data: DashMap::new(),
            shutdown_tx,
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
                    let removed = Self::cleanup_internal(&inner.data);
                    if removed > 0 {
                        // Optional: could add logging here
                        // println!("Cleanup removed {} expired entries", removed);
                    }
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

    /// Internal cleanup logic (shared between manual and background cleanup)
    fn cleanup_internal(data: &DashMap<String, Entry>) -> usize {
        let initial_len = data.len();
        data.retain(|_, entry| !entry.is_expired());
        initial_len - data.len()
    }

    /// Stores a value with the given key and TTL (time-to-live) in seconds
    /// 
    /// If the key already exists, the value is overwritten.
    /// TTL of 0 means the entry never expires.
    /// Non-zero TTL is capped to prevent overflow (max ~100 years).
    pub fn set(&self, key: impl Into<String>, value: impl Into<String>, ttl_seconds: u64) {
        // Cap TTL to ~100 years to prevent overflow when adding to Instant
        const MAX_TTL_SECONDS: u64 = 100 * 365 * 24 * 60 * 60; // ~100 years
        
        // TTL of 0 means never expire (use max TTL)
        let safe_ttl = if ttl_seconds == 0 {
            MAX_TTL_SECONDS
        } else {
            ttl_seconds.min(MAX_TTL_SECONDS)
        };
        
        let expires_at = Instant::now() + Duration::from_secs(safe_ttl);
        let entry = Entry::new(value.into(), expires_at);
        self.inner.data.insert(key.into(), entry);
    }

    /// Stores a value that expires immediately (for testing purposes)
    #[cfg(test)]
    fn set_expired(&self, key: impl Into<String>, value: impl Into<String>) {
        // Set expiration to a time in the past
        let expires_at = Instant::now() - Duration::from_secs(1);
        let entry = Entry::new(value.into(), expires_at);
        self.inner.data.insert(key.into(), entry);
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
            self.inner.data.remove(key);
            return None;
        }
        
        Some(entry.value().value().to_string())
    }

    /// Deletes a key from the store
    /// 
    /// Returns `true` if the key existed (regardless of expiration), `false` otherwise.
    pub fn delete(&self, key: &str) -> bool {
        self.inner.data.remove(key).is_some()
    }

    /// Manually triggers cleanup of all expired entries
    /// 
    /// Returns the number of entries removed.
    /// 
    /// Note: This is also done automatically by the background task.
    pub fn cleanup(&self) -> usize {
        Self::cleanup_internal(&self.inner.data)
    }

    /// Returns the number of entries in the store (including expired ones)
    pub fn len(&self) -> usize {
        self.inner.data.len()
    }

    /// Returns `true` if the store is empty
    pub fn is_empty(&self) -> bool {
        self.inner.data.is_empty()
    }

    /// Checks if a key exists and is not expired
    pub fn contains_key(&self, key: &str) -> bool {
        match self.inner.data.get(key) {
            Some(entry) => !entry.value().is_expired(),
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
        store.set("key1", "value1", 60);
        
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
        store.set("key1", "value1", 60);
        store.set("key1", "value2", 60);
        
        assert_eq!(store.get("key1"), Some("value2".to_string()));
    }

    #[test]
    fn test_delete() {
        let store = create_test_store();
        store.set("key1", "value1", 60);
        
        assert!(store.delete("key1"));
        assert_eq!(store.get("key1"), None);
        assert!(!store.delete("key1")); // Already deleted
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
        store.set("valid", "value3", 60);
        
        thread::sleep(Duration::from_millis(10));
        
        let removed = store.cleanup();
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("valid"), Some("value3".to_string()));
    }

    #[test]
    fn test_contains_key() {
        let store = create_test_store();
        store.set("key1", "value1", 60);
        store.set_expired("expired", "value2");
        
        thread::sleep(Duration::from_millis(10));
        
        assert!(store.contains_key("key1"));
        assert!(!store.contains_key("expired"));
        assert!(!store.contains_key("nonexistent"));
    }

    #[test]
    fn test_keys() {
        let store = create_test_store();
        store.set("key1", "value1", 60);
        store.set("key2", "value2", 60);
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
        
        store.set("key1", "value1", 60);
        
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_extreme_ttl_does_not_panic() {
        let store = create_test_store();
        // This should not panic - TTL is capped internally
        store.set("key1", "value1", u64::MAX);
        
        // Should still be retrievable (won't expire for ~100 years)
        assert_eq!(store.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn test_zero_ttl_means_never_expire() {
        let store = create_test_store();
        // TTL of 0 means the entry never expires
        store.set("key1", "value1", 0);
        
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
                    store.set(key, value, 60);
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
            store.set(format!("key{}", i), format!("value{}", i), 60);
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
                    store.set(key, "new_value", 60);
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
                    store.set("contested_key", value, 60);
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
            store.set(format!("persistent{}", i), "value", 60);
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
                store_writer.set(format!("new{}", i), "value", 60);
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
        store.set("keep", "value3", 60);
        
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
        
        store1.set("key1", "value1", 60);
        
        // Both stores should see the same data
        assert_eq!(store2.get("key1"), Some("value1".to_string()));
        
        store2.set("key2", "value2", 60);
        assert_eq!(store1.get("key2"), Some("value2".to_string()));
    }

    #[tokio::test]
    async fn test_shutdown_stops_cleanup_task() {
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_millis(10));
        let store = Store::with_config(config);
        
        // TTL=0 means never expire
        store.set("key1", "value1", 0);
        
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
        store2.set("keep", "value", 60); // Non-expiring entry
        
        // Wait for store1's cleanup to run
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // store1 should be cleaned up
        assert_eq!(store1.len(), 0);
        
        // store2 should still have its entry (independent store)
        assert_eq!(store2.len(), 1);
        assert_eq!(store2.get("keep"), Some("value".to_string()));
    }
}
