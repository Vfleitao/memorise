use bytes::Bytes;
use dashmap::mapref::entry::Entry as MapEntry;
use dashmap::DashMap;
use std::cell::Cell;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::config::{EvictionPolicy, StoreConfig};
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
    /// Current total size of stored data in bytes (keys + values + `ENTRY_OVERHEAD` per entry)
    current_size: AtomicUsize,
    /// Maximum storage size in bytes (0 = unlimited)
    max_storage_bytes: usize,
    /// What `set` does when the storage limit is reached
    eviction_policy: EvictionPolicy,
    /// Reference point for entries' last access times
    epoch: Instant,
}

/// Default limit for search_keys operations
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// Maximum limit for search_keys operations (hard cap)
pub const MAX_SEARCH_LIMIT: usize = 250;

/// Maximum number of keys to scan during search operations.
/// Prevents unbounded memory allocation from a single search request.
pub const MAX_SEARCH_SCAN: usize = 10_000;

/// Approximate memory used by one entry on top of its key and value bytes:
/// the map slot (key `String` + `Entry`), the hash table's control byte and
/// the value buffer's shared reference-count header. Allocator rounding and spare
/// hash table capacity are not included, so real memory use is still somewhat higher.
pub const ENTRY_OVERHEAD: usize =
    std::mem::size_of::<(String, Entry)>() + 1 + 3 * std::mem::size_of::<usize>();

/// Number of entries compared when choosing an eviction victim.
/// Same default as Redis' `maxmemory-samples`.
const EVICTION_SAMPLES: usize = 5;

/// Cheap per-thread pseudo-random numbers for eviction sampling (xorshift64)
fn random_usize() -> usize {
    thread_local! {
        static STATE: Cell<u64> = Cell::new(RandomState::new().hash_one(0u8) | 1);
    }
    STATE.with(|state| {
        let mut x = state.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        x as usize
    })
}

/// Thread-safe in-memory key-value store with TTL support
///
/// Uses `DashMap` for lock-free concurrent access. Reads never block other reads,
/// and writes only block access to the specific key being written.
///
/// Each store spawns its own background cleanup task that periodically removes
/// expired entries. The cleanup task is automatically stopped when the last
/// handle to the store is dropped.
///
/// # Storage Limits
///
/// The store has a configurable maximum storage size (default: 100MB). Each
/// entry counts as its key and value bytes plus `ENTRY_OVERHEAD`.
///
/// When a write would go over the limit, the configured `EvictionPolicy` applies:
/// - `Lru` (default): entries are evicted to make room, expired ones first and then
///   the least recently used, chosen by sampling a few entries like Redis does.
/// - `NoEviction`: the write is rejected with `SetError::StorageFull`.
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
            eviction_policy: config.eviction_policy,
            epoch: Instant::now(),
        });

        // Spawn the background cleanup task. It only holds a weak reference so
        // that dropping the last `Store` handle drops `StoreInner`, which signals
        // the task to stop.
        tokio::spawn(Self::cleanup_task(
            Arc::downgrade(&inner),
            config.cleanup_interval,
            shutdown_rx,
        ));

        Self { inner }
    }

    /// Background task that periodically cleans up expired entries
    async fn cleanup_task(
        inner: Weak<StoreInner>,
        interval: Duration,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick - we want to wait for the interval first
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    // The store is gone if the upgrade fails
                    let Some(inner) = inner.upgrade() else { break };
                    Self::cleanup_internal(&inner);
                }
                changed = shutdown_rx.changed() => {
                    // An error means the sender was dropped along with the store
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    }

    /// Calculate the accounted size of a key-value pair in bytes
    fn entry_size(key: &str, value: &[u8]) -> usize {
        key.len() + value.len() + ENTRY_OVERHEAD
    }

    /// Converts `now` to the coarse clock used for entries' last access times
    fn clock(&self, now: Instant) -> u32 {
        now.saturating_duration_since(self.inner.epoch)
            .as_secs()
            .min(u32::MAX as u64) as u32
    }

    /// Subtracts freed bytes from the size counter
    fn release(&self, bytes: usize) {
        if bytes > 0 {
            self.inner.current_size.fetch_sub(bytes, Ordering::Relaxed);
        }
    }

    /// Adjusts the size counter for replacing an entry of `old_size` bytes
    /// (0 for a new key) with one of `new_size` bytes.
    ///
    /// Growth is only applied if it fits within the limit, using a
    /// compare-and-swap so concurrent writers can't overshoot it together.
    /// Returns `false` if it doesn't fit.
    fn try_reserve(&self, new_size: usize, old_size: usize) -> bool {
        if new_size <= old_size {
            self.release(old_size - new_size);
            return true;
        }

        let growth = new_size - old_size;
        let max = self.inner.max_storage_bytes;
        if max == 0 {
            self.inner.current_size.fetch_add(growth, Ordering::Relaxed);
            return true;
        }

        // A manual CAS loop rather than `fetch_update`, which newer toolchains
        // deprecate but whose replacement isn't available on the Docker image's Rust 1.85
        let mut current = self.inner.current_size.load(Ordering::Relaxed);
        loop {
            let next = match current.checked_add(growth) {
                Some(next) if next <= max => next,
                _ => return false,
            };
            match self.inner.current_size.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    fn storage_full(&self) -> SetError {
        SetError::StorageFull {
            current_bytes: self.size_bytes(),
            max_bytes: self.inner.max_storage_bytes,
        }
    }

    /// Internal cleanup logic (shared between manual and background cleanup)
    ///
    /// Each shard is scanned under a read lock, so readers aren't blocked, and
    /// only the expired keys found are then removed one at a time.
    fn cleanup_internal(inner: &StoreInner) -> usize {
        let now = Instant::now();
        let mut removed_count = 0;
        let mut removed_bytes = 0usize;
        let mut expired_keys = Vec::new();

        for shard in inner.data.shards() {
            {
                let table = shard.read();
                // SAFETY: the read guard keeps the table alive and unmodified for
                // the whole iteration, and no bucket reference outlives it.
                unsafe {
                    for bucket in table.iter() {
                        let (key, value) = bucket.as_ref();
                        if value.get().is_expired_at(now) {
                            expired_keys.push(key.clone());
                        }
                    }
                }
            }

            // Re-check expiry so entries overwritten since the scan are kept
            for key in expired_keys.drain(..) {
                if let Some((k, entry)) = inner.data.remove_if(&key, |_, v| v.is_expired_at(now)) {
                    removed_bytes += Self::entry_size(&k, entry.value());
                    removed_count += 1;
                }
            }
        }

        if removed_bytes > 0 {
            inner.current_size.fetch_sub(removed_bytes, Ordering::Relaxed);
        }

        removed_count
    }

    /// Evicts one entry to free space, chosen by sampled LRU like Redis does.
    ///
    /// Looks at `EVICTION_SAMPLES` neighbouring entries from a random position in
    /// a random shard (continuing into further shards if that one has fewer) and
    /// removes an expired one if there is one, otherwise the least recently used.
    ///
    /// Returns `false` if there was nothing to evict.
    fn evict_one(&self, now: Instant) -> bool {
        let shards = self.inner.data.shards();
        let first_shard = random_usize() % shards.len();
        let mut sampled = 0;
        // Expired entries rank first (`false < true`), then the oldest access time
        let mut victim: Option<((bool, u32), String)> = None;

        for offset in 0..shards.len() {
            let table = shards[(first_shard + offset) % shards.len()].read();
            if table.is_empty() {
                continue;
            }

            // Always a power of two
            let buckets = table.buckets();
            let start = random_usize() & (buckets - 1);
            for i in 0..buckets {
                let index = (start + i) & (buckets - 1);
                // SAFETY: `index < buckets`, and the read guard keeps the table
                // unmodified while the bucket reference is used in this iteration.
                if !unsafe { table.is_bucket_full(index) } {
                    continue;
                }
                let (key, value) = unsafe { table.bucket(index).as_ref() };

                let entry = value.get();
                let rank = (!entry.is_expired_at(now), entry.last_access());
                if victim.as_ref().is_none_or(|(best, _)| rank < *best) {
                    victim = Some((rank, key.clone()));
                }

                sampled += 1;
                if sampled == EVICTION_SAMPLES {
                    break;
                }
            }

            if sampled == EVICTION_SAMPLES {
                break;
            }
        }

        let Some((rank, key)) = victim else {
            return false;
        };

        // Skip the victim if it was read or rewritten since it was sampled. The
        // caller retries with a fresh sample either way.
        let removed = self.inner.data.remove_if(&key, |_, entry| {
            (!entry.is_expired_at(now), entry.last_access()) <= rank
        });
        if let Some((k, entry)) = removed {
            self.release(Self::entry_size(&k, entry.value()));
        }

        true
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
    /// # Storage Limits
    ///
    /// If the write would exceed the storage limit, entries are evicted to make
    /// room under `EvictionPolicy::Lru`, or the write is rejected under
    /// `EvictionPolicy::NoEviction`. The limit is never exceeded.
    ///
    /// # Errors
    ///
    /// Returns `SetError::StorageFull` if the entry doesn't fit: either the policy is
    /// `NoEviction`, or the entry is larger than the whole storage limit.
    pub fn set(&self, key: impl Into<String>, value: impl Into<Bytes>, ttl_seconds: u64) -> Result<(), SetError> {
        let mut key = key.into();
        let value: Bytes = value.into();
        let new_size = Self::entry_size(&key, &value);

        // An entry bigger than the whole store can never fit, so don't evict anything for it
        let max = self.inner.max_storage_bytes;
        if max > 0 && new_size > max {
            return Err(self.storage_full());
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

        let now = Instant::now();
        let entry = Entry::new(value, now + Duration::from_secs(safe_ttl), self.clock(now));

        loop {
            // The shard stays write-locked from the lookup to the insert, so the
            // size counter is updated together with the map.
            match self.inner.data.entry(key) {
                MapEntry::Occupied(mut occupied) => {
                    let old_size = Self::entry_size(occupied.key(), occupied.get().value());
                    if self.try_reserve(new_size, old_size) {
                        occupied.insert(entry);
                        return Ok(());
                    }
                    key = occupied.into_key();
                }
                MapEntry::Vacant(vacant) => {
                    if self.try_reserve(new_size, 0) {
                        vacant.insert(entry);
                        return Ok(());
                    }
                    key = vacant.into_key();
                }
            }

            // Out of space. The shard lock has been released, so evicting
            // (which locks shards itself) can't deadlock.
            if self.inner.eviction_policy == EvictionPolicy::NoEviction || !self.evict_one(now) {
                return Err(self.storage_full());
            }
        }
    }

    /// Stores a value that expires immediately (for testing purposes)
    #[cfg(test)]
    fn set_expired(&self, key: impl Into<String>, value: impl Into<Bytes>) {
        let key = key.into();
        let value: Bytes = value.into();
        let entry_size = Self::entry_size(&key, &value);

        // Set expiration to a time in the past
        let now = Instant::now();
        let entry = Entry::new(value, now - Duration::from_secs(1), self.clock(now));
        self.inner.data.insert(key, entry);
        self.inner.current_size.fetch_add(entry_size, Ordering::Relaxed);
    }

    /// Retrieves a value by key
    ///
    /// Returns `None` if the key doesn't exist or has expired.
    /// Expired entries are automatically removed. A successful read counts as
    /// an access for LRU eviction.
    ///
    /// Returns a `Bytes` handle that shares the stored buffer: getting a value
    /// is a reference-count bump, never a copy.
    pub fn get(&self, key: &str) -> Option<Bytes> {
        let now = Instant::now();
        let entry = self.inner.data.get(key)?;

        if entry.value().is_expired_at(now) {
            // Drop the read reference before removing
            drop(entry);
            // Use remove_if to atomically verify expiration and remove.
            // This prevents a race where another thread replaces the entry
            // between our check and removal, which would cause size drift.
            if let Some((k, removed)) = self.inner.data.remove_if(key, |_, v| v.is_expired_at(now)) {
                self.release(Self::entry_size(&k, removed.value()));
            }
            return None;
        }

        entry.value().touch(self.clock(now));
        Some(entry.value().value_shared())
    }

    /// Deletes a key from the store
    ///
    /// Returns `true` if the key existed (regardless of expiration), `false` otherwise.
    #[must_use = "returns whether the key existed"]
    pub fn delete(&self, key: &str) -> bool {
        if let Some((k, entry)) = self.inner.data.remove(key) {
            self.release(Self::entry_size(&k, entry.value()));
            true
        } else {
            false
        }
    }

    /// Deletes all entries from the store.
    ///
    /// Returns the number of entries that were removed.
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
    /// Shards are cleared one at a time, so entries written concurrently to a
    /// shard that has already been cleared survive. The size counter stays
    /// accurate either way, since only the removed entries are subtracted.
    pub fn delete_all(&self) -> usize {
        let mut removed_count = 0;
        let mut removed_bytes = 0usize;

        self.inner.data.retain(|key, entry| {
            removed_bytes += Self::entry_size(key, entry.value());
            removed_count += 1;
            false
        });

        self.release(removed_bytes);
        removed_count
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

    /// Returns the current storage size in bytes, including `ENTRY_OVERHEAD` per entry
    pub fn size_bytes(&self) -> usize {
        self.inner.current_size.load(Ordering::Relaxed)
    }

    /// Returns the maximum storage size in bytes (0 = unlimited)
    pub fn max_size_bytes(&self) -> usize {
        self.inner.max_storage_bytes
    }

    /// Returns the policy applied when the storage limit is reached
    pub fn eviction_policy(&self) -> EvictionPolicy {
        self.inner.eviction_policy
    }

    /// Checks if a key exists and is not expired.
    ///
    /// Expired entries are lazily removed when checked. Unlike `get`, this
    /// doesn't count as an access for LRU eviction.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        let now = Instant::now();
        match self.inner.data.get(key) {
            Some(entry) => {
                if entry.value().is_expired_at(now) {
                    // Drop the read reference before removing
                    drop(entry);
                    // Use remove_if to atomically verify expiration and remove.
                    // This prevents a race where another thread replaces the entry
                    // between our check and removal, which would cause size drift.
                    if let Some((k, removed)) = self.inner.data.remove_if(key, |_, v| v.is_expired_at(now)) {
                        self.release(Self::entry_size(&k, removed.value()));
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
        self.keys_with_limit(None)
    }

    /// Returns keys that are not expired, with an optional limit
    ///
    /// # Arguments
    /// * `limit` - Maximum number of keys to return. None means no limit.
    pub fn keys_with_limit(&self, limit: Option<usize>) -> Vec<String> {
        let now = Instant::now();
        let iter = self.inner.data
            .iter()
            .filter(|entry| !entry.value().is_expired_at(now))
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
        let now = Instant::now();

        // Collect matching, non-expired keys with a scan cap to bound memory allocation.
        // Filter by prefix first to avoid cloning keys that don't match,
        // then check expiration.
        // The cap prevents a single request from causing OOM on large stores.
        let mut matching_keys: Vec<String> = self.inner.data
            .iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .filter(|entry| !entry.value().is_expired_at(now))
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

        assert_eq!(store.get("key1").as_deref(), Some(&b"value1"[..]));
    }

    #[test]
    fn test_binary_values_round_trip() {
        let store = create_test_store();
        // Every byte value, including sequences that aren't valid UTF-8
        let value: Vec<u8> = (0..=255u8).collect();
        store.set("binary", value.clone(), 60).unwrap();

        assert_eq!(store.get("binary").as_deref(), Some(value.as_slice()));
        assert_eq!(store.size_bytes(), "binary".len() + 256 + ENTRY_OVERHEAD);
    }

    #[test]
    fn test_empty_value_is_stored() {
        let store = create_test_store();
        store.set("empty", Vec::new(), 60).unwrap();

        assert_eq!(store.get("empty").as_deref(), Some(&b""[..]));
    }

    #[test]
    fn test_get_shares_the_stored_buffer() {
        let store = create_test_store();
        store.set("key", b"shared".to_vec(), 60).unwrap();

        // Each get hands out the same buffer rather than a copy
        let first = store.get("key").unwrap();
        let second = store.get("key").unwrap();
        assert_eq!(first.as_ptr(), second.as_ptr());
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

        assert_eq!(store.get("key1").as_deref(), Some(&b"value2"[..]));
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
        // Room for exactly two entries
        let store = create_test_store_with_config(no_eviction(sz("key1", "value1") * 2));

        // Fill up storage
        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        assert!(store.set("key3", "value3", 60).is_err());

        // Delete all should reset size to 0
        store.delete_all();
        assert_eq!(store.size_bytes(), 0);

        // Should be able to fill it again (storage limit reset)
        store.set("new1", "value1", 60).unwrap();
        store.set("new2", "value2", 60).unwrap();
        assert_eq!(store.size_bytes(), sz("new1", "value1") * 2);
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
        assert_eq!(store.get("valid").as_deref(), Some(&b"value3"[..]));
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
        assert_eq!(store.get("key1").as_deref(), Some(&b"value1"[..]));
    }

    #[test]
    fn test_zero_ttl_means_never_expire() {
        let store = create_test_store();
        // TTL of 0 means the entry never expires
        store.set("key1", "value1", 0).unwrap();

        // Wait a bit to ensure it doesn't expire
        thread::sleep(Duration::from_millis(50));

        // Should still be retrievable
        assert_eq!(store.get("key1").as_deref(), Some(&b"value1"[..]));
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
        assert_eq!(store.get("keep").as_deref(), Some(&b"value3"[..]));
    }

    #[tokio::test]
    async fn test_store_clone_shares_data() {
        let store1 = Store::new();
        let store2 = store1.clone();
        
        store1.set("key1", "value1", 60).unwrap();

        // Both stores should see the same data
        assert_eq!(store2.get("key1").as_deref(), Some(&b"value1"[..]));

        store2.set("key2", "value2", 60).unwrap();
        assert_eq!(store1.get("key2").as_deref(), Some(&b"value2"[..]));
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
        assert_eq!(store.get("key1").as_deref(), Some(&b"value1"[..]));
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
        assert_eq!(store2.get("keep").as_deref(), Some(&b"value"[..]));
    }

    // === storage limit tests ===

    /// Accounted size of an entry, matching `Store::entry_size`
    fn sz(key: &str, value: impl AsRef<[u8]>) -> usize {
        key.len() + value.as_ref().len() + ENTRY_OVERHEAD
    }

    /// Config with a byte limit that rejects writes when full
    fn no_eviction(max_bytes: usize) -> StoreConfig {
        StoreConfig::default()
            .with_max_storage_bytes(max_bytes)
            .with_eviction_policy(EvictionPolicy::NoEviction)
            .with_cleanup_interval(Duration::from_secs(3600))
    }

    /// Config with a byte limit that evicts by LRU when full
    fn lru(max_bytes: usize) -> StoreConfig {
        StoreConfig::default()
            .with_max_storage_bytes(max_bytes)
            .with_eviction_policy(EvictionPolicy::Lru)
            .with_cleanup_interval(Duration::from_secs(3600))
    }

    /// Sum of the accounted sizes of everything actually in the store
    fn actual_size(store: &Store) -> usize {
        store.inner.data
            .iter()
            .map(|entry| sz(entry.key(), entry.value().value()))
            .sum()
    }

    /// Sets an entry's last access time directly, since the real clock only ticks once a second
    fn set_last_access(store: &Store, key: &str, seconds: u32) {
        store.inner.data.get(key).unwrap().touch(seconds);
    }

    #[test]
    fn test_entry_overhead_is_counted() {
        // Guard against the overhead silently becoming 0
        assert!(ENTRY_OVERHEAD >= std::mem::size_of::<String>() + std::mem::size_of::<Entry>());
    }

    #[test]
    fn test_storage_size_tracking() {
        let config = StoreConfig::default().with_max_storage_mb(0); // Unlimited
        let store = create_test_store_with_config(config);

        assert_eq!(store.size_bytes(), 0);

        store.set("key1", "value1", 60).unwrap();
        assert_eq!(store.size_bytes(), sz("key1", "value1"));

        store.set("key2", "value2", 60).unwrap();
        assert_eq!(store.size_bytes(), sz("key1", "value1") + sz("key2", "value2"));

        // Overwrite with smaller value
        store.set("key1", "x", 60).unwrap();
        assert_eq!(store.size_bytes(), sz("key1", "x") + sz("key2", "value2"));

        // Delete
        let _ = store.delete("key1");
        assert_eq!(store.size_bytes(), sz("key2", "value2"));

        let _ = store.delete("key2");
        assert_eq!(store.size_bytes(), 0);
    }

    #[test]
    fn test_storage_limit_enforced() {
        // Room for exactly 5 entries of this size
        let entry = sz("key1", "value1");
        let store = create_test_store_with_config(no_eviction(entry * 5));

        for i in 1..=5 {
            store.set(format!("key{}", i), format!("value{}", i), 60).unwrap();
        }
        assert_eq!(store.size_bytes(), entry * 5); // Exactly at limit

        // This should fail - would exceed the limit
        let result = store.set("key6", "value6", 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));

        // Verify the failed insert didn't add the key
        assert!(!store.contains_key("key6"));
        assert_eq!(store.len(), 5);
        assert_eq!(store.size_bytes(), entry * 5);
    }

    #[test]
    fn test_storage_limit_allows_overwrites() {
        let entry = sz("key1", "value1");
        let store = create_test_store_with_config(no_eviction(entry * 3));

        store.set("key1", "value1", 60).unwrap();
        store.set("key2", "value2", 60).unwrap();
        store.set("key3", "value3", 60).unwrap(); // At limit

        // Overwriting an existing key with same size should work
        store.set("key1", "valueX", 60).unwrap();

        // Overwriting with smaller value should work, freeing 5 bytes
        store.set("key1", "x", 60).unwrap();
        assert_eq!(store.size_bytes(), entry * 3 - 5);

        // Growing another entry by those 5 bytes fits exactly
        store.set("key2", "value2-more", 60).unwrap();
        assert_eq!(store.size_bytes(), entry * 3);

        // Growing by one more byte doesn't
        let result = store.set("key3", "value3+", 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));
        assert_eq!(store.get("key3").as_deref(), Some(&b"value3"[..]));
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_storage_freed_on_expiry_cleanup() {
        let store = create_test_store_with_config(no_eviction(10_000));

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
        assert_eq!(size_after, sz("keep", "value3"));
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
        // Room for 12 entries plus 4 spare bytes
        let entry = sz("k00", "val00");
        let max = entry * 12 + 4;
        let store = create_test_store_with_config(no_eviction(max));

        for i in 0..12 {
            store.set(format!("k{:02}", i), format!("val{:02}", i), 60).unwrap();
        }
        assert_eq!(store.size_bytes(), entry * 12);

        // Try to add more - should fail with StorageFull
        let result = store.set("overflow", "data", 60);

        match result {
            Err(SetError::StorageFull { current_bytes, max_bytes }) => {
                assert_eq!(max_bytes, max);
                assert_eq!(current_bytes, entry * 12);
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
        let config = StoreConfig::default()
            .with_max_storage_mb(1)
            .with_eviction_policy(EvictionPolicy::NoEviction);
        let store = create_test_store_with_config(config);

        assert_eq!(store.max_size_bytes(), 1024 * 1024);

        // Create a large value (~100KB)
        let large_value = "x".repeat(100_000);

        // Should be able to add ~10 of these
        for i in 0..10 {
            store.set(format!("large{}", i), large_value.clone(), 60).unwrap();
        }

        // Size should be approximately 10 * (6 + 100_000 + overhead)
        let size = store.size_bytes();
        assert!(size > 1_000_000, "Should have stored ~1MB");
        assert!(size < 1_048_576, "Should still be under limit");

        // One more should fail
        let result = store.set("large10", large_value, 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));
    }

    #[test]
    fn test_storage_limit_delete_frees_space_for_new_entries() {
        // Room for 5 entries plus 3 spare bytes
        let entry = sz("key0", "val00");
        let store = create_test_store_with_config(no_eviction(entry * 5 + 3));

        for i in 0..5 {
            store.set(format!("key{}", i), format!("val{:02}", i), 60).unwrap();
        }
        assert_eq!(store.size_bytes(), entry * 5);

        // Can't add more: needs entry + 3 bytes, only 3 free
        let result = store.set("newkey", "value!", 60);
        assert!(matches!(result, Err(SetError::StorageFull { .. })));

        // Delete one entry
        let _ = store.delete("key0");
        assert_eq!(store.size_bytes(), entry * 4);

        // Now the new entry fits exactly
        store.set("newkey", "value!", 60).unwrap();
        assert_eq!(store.size_bytes(), entry * 5 + 3);
        assert!(store.contains_key("newkey"));
    }

    #[test]
    fn test_storage_limit_is_exact_under_concurrent_writes() {
        // Room for exactly 50 entries; 8 threads race to write 800
        let entry = sz("t0:k000", "value");
        let store = Arc::new(create_test_store_with_config(no_eviction(entry * 50)));
        let successes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..8)
            .map(|t| {
                let store = Arc::clone(&store);
                let successes = Arc::clone(&successes);
                thread::spawn(move || {
                    for i in 0..100 {
                        if store.set(format!("t{}:k{:03}", t, i), "value", 60).is_ok() {
                            successes.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        assert_eq!(successes.load(Ordering::Relaxed), 50);
        assert_eq!(store.len(), 50);
        assert_eq!(store.size_bytes(), entry * 50);
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_size_stays_accurate_when_delete_all_races_writes() {
        use std::sync::atomic::AtomicBool;

        let store = Arc::new(create_test_store_with_config(StoreConfig::default().with_max_storage_mb(0)));
        let done = Arc::new(AtomicBool::new(false));

        // Writers keep going for as long as the clearer runs
        let writers: Vec<_> = (0..4)
            .map(|t| {
                let store = Arc::clone(&store);
                let done = Arc::clone(&done);
                thread::spawn(move || {
                    let mut i = 0usize;
                    while !done.load(Ordering::Relaxed) {
                        // Overwrite a small set of keys with varying sizes
                        let value = "v".repeat(i % 17);
                        store.set(format!("t{}:k{}", t, i % 50), value, 60).unwrap();
                        if i % 7 == 3 {
                            let _ = store.delete(&format!("t{}:k{}", t, (i + 3) % 50));
                        }
                        i += 1;
                    }
                })
            })
            .collect();

        for _ in 0..5_000 {
            store.delete_all();
        }
        done.store(true, Ordering::Relaxed);

        for handle in writers {
            handle.join().expect("Thread panicked");
        }

        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_delete_all_returns_exact_count() {
        let store = create_test_store();
        for i in 0..10 {
            store.set(format!("key{}", i), "value", 60).unwrap();
        }
        store.set_expired("expired", "value");

        // Expired-but-not-yet-cleaned entries are removed and counted too
        assert_eq!(store.delete_all(), 11);
        assert_eq!(store.size_bytes(), 0);
    }

    // === eviction tests ===

    #[test]
    fn test_lru_evicts_least_recently_used() {
        let entry = sz("a", "value");
        let store = create_test_store_with_config(lru(entry * 3));

        store.set("a", "value", 60).unwrap();
        store.set("b", "value", 60).unwrap();
        store.set("c", "value", 60).unwrap();
        set_last_access(&store, "a", 10);
        set_last_access(&store, "b", 1);
        set_last_access(&store, "c", 10);

        // Full, so "b" (least recently used) makes room for "d"
        store.set("d", "value", 60).unwrap();

        assert!(store.contains_key("a"));
        assert!(!store.contains_key("b"));
        assert!(store.contains_key("c"));
        assert!(store.contains_key("d"));
        assert_eq!(store.size_bytes(), entry * 3);
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_get_refreshes_recency() {
        let entry = sz("a", "value");
        let store = create_test_store_with_config(lru(entry * 3));

        store.set("a", "value", 60).unwrap();
        store.set("b", "value", 60).unwrap();
        store.set("c", "value", 60).unwrap();

        // The access clock has one-second resolution
        thread::sleep(Duration::from_millis(1100));
        assert!(store.get("a").is_some());
        assert!(store.get("c").is_some());
        // contains_key doesn't count as an access
        assert!(store.contains_key("b"));

        store.set("d", "value", 60).unwrap();

        assert!(store.contains_key("a"));
        assert!(!store.contains_key("b"));
        assert!(store.contains_key("c"));
        assert!(store.contains_key("d"));
    }

    #[test]
    fn test_eviction_prefers_expired_entries() {
        let entry = sz("a", "value");
        let store = create_test_store_with_config(lru(entry * 3));

        store.set("a", "value", 60).unwrap();
        store.set("b", "value", 60).unwrap();
        store.set_expired("c", "value");
        // The expired entry was used most recently, but goes first anyway
        set_last_access(&store, "c", 100);

        store.set("d", "value", 60).unwrap();

        assert!(store.contains_key("a"));
        assert!(store.contains_key("b"));
        assert!(store.inner.data.get("c").is_none());
        assert!(store.contains_key("d"));
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_eviction_frees_enough_for_large_value() {
        let small = sz("key00", "value");
        let max = small * 10;
        let store = create_test_store_with_config(lru(max));

        for i in 0..10 {
            store.set(format!("key{:02}", i), "value", 60).unwrap();
        }

        // Needs roughly three small entries' worth of space
        let big_value = "x".repeat(small * 3 - ENTRY_OVERHEAD - 3);
        store.set("big", big_value.clone(), 60).unwrap();

        assert_eq!(store.get("big").as_deref(), Some(big_value.as_bytes()));
        assert!(store.len() <= 8);
        assert!(store.size_bytes() <= max);
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_overwrite_when_full_evicts_others() {
        let entry = sz("a", "value");
        let store = create_test_store_with_config(lru(entry * 3));

        store.set("a", "value", 60).unwrap();
        store.set("b", "value", 60).unwrap();
        store.set("c", "value", 60).unwrap();
        set_last_access(&store, "a", 10);
        set_last_access(&store, "b", 1);
        set_last_access(&store, "c", 10);

        // Growing "a" by one byte needs room, and "b" is the oldest
        store.set("a", "value+", 60).unwrap();

        assert_eq!(store.get("a").as_deref(), Some(&b"value+"[..]));
        assert!(!store.contains_key("b"));
        assert!(store.contains_key("c"));
        assert_eq!(store.size_bytes(), actual_size(&store));
    }

    #[test]
    fn test_entry_larger_than_limit_is_rejected_without_evicting() {
        let entry = sz("a", "value");
        let store = create_test_store_with_config(lru(entry * 3));

        store.set("a", "value", 60).unwrap();
        store.set("b", "value", 60).unwrap();
        store.set("c", "value", 60).unwrap();

        let huge = "x".repeat(entry * 3);
        let result = store.set("huge", huge, 60);

        assert!(matches!(result, Err(SetError::StorageFull { .. })));
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_lru_keeps_store_within_limit_under_concurrent_writes() {
        let entry = sz("t0:k0000", "value");
        let max = entry * 100;
        let store = Arc::new(create_test_store_with_config(lru(max)));

        let handles: Vec<_> = (0..8)
            .map(|t| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    for i in 0..1_000 {
                        store.set(format!("t{}:k{:04}", t, i), "value", 60).unwrap();
                        let _ = store.get(&format!("t{}:k{:04}", t, i / 2));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        assert!(store.size_bytes() <= max);
        assert_eq!(store.size_bytes(), actual_size(&store));
        assert!(!store.is_empty());
    }

    #[tokio::test]
    async fn test_dropping_store_stops_cleanup_task() {
        let metrics = tokio::runtime::Handle::current().metrics();
        let tasks_before = metrics.num_alive_tasks();

        let store = Store::with_config(
            StoreConfig::default().with_cleanup_interval(Duration::from_millis(10)),
        );
        let weak = Arc::downgrade(&store.inner);
        let clone = store.clone();
        assert_eq!(metrics.num_alive_tasks(), tasks_before + 1);

        // Let the cleanup task tick a few times
        tokio::time::sleep(Duration::from_millis(30)).await;

        drop(store);
        assert!(weak.upgrade().is_some(), "a clone still holds the store");

        drop(clone);
        // The cleanup task must not keep the store alive
        assert!(weak.upgrade().is_none());

        // And it exits once the store is gone
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(metrics.num_alive_tasks(), tasks_before);
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
