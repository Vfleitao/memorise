use std::str::FromStr;
use std::time::Duration;

/// What the store does when a write would exceed the storage limit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvictionPolicy {
    /// Evict entries to make room: expired entries first, then the least
    /// recently used ones (approximated by sampling, like Redis' `allkeys-lru`)
    #[default]
    Lru,
    /// Reject the write with `SetError::StorageFull`
    NoEviction,
}

impl FromStr for EvictionPolicy {
    type Err = String;

    /// Parses `lru` or `noeviction` (case-insensitive)
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lru" => Ok(Self::Lru),
            "noeviction" => Ok(Self::NoEviction),
            other => Err(format!(
                "unknown eviction policy '{}' (expected 'lru' or 'noeviction')",
                other
            )),
        }
    }
}

impl std::fmt::Display for EvictionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lru => f.write_str("lru"),
            Self::NoEviction => f.write_str("noeviction"),
        }
    }
}

/// Configuration for the store
/// 
/// # Example
/// 
/// ```rust
/// use memorize_core::StoreConfig;
/// use std::time::Duration;
/// 
/// let config = StoreConfig::default()
///     .with_cleanup_interval(Duration::from_secs(30))
///     .with_max_storage_mb(256);
/// ```
#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Interval between cleanup runs (default: 60 seconds)
    pub cleanup_interval: Duration,
    /// Maximum storage size in bytes (default: 100MB, 0 = unlimited)
    pub max_storage_bytes: usize,
    /// What to do when the storage limit is reached (default: LRU eviction)
    pub eviction_policy: EvictionPolicy,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            cleanup_interval: Duration::from_secs(60),
            max_storage_bytes: 100 * 1024 * 1024, // 100MB default
            eviction_policy: EvictionPolicy::Lru,
        }
    }
}

impl StoreConfig {
    /// Creates a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cleanup interval
    /// 
    /// This determines how often the background task runs to remove expired entries.
    /// 
    /// # Arguments
    /// 
    /// * `interval` - The duration between cleanup runs
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use memorize_core::StoreConfig;
    /// use std::time::Duration;
    /// 
    /// // Cleanup every 30 seconds
    /// let config = StoreConfig::default()
    ///     .with_cleanup_interval(Duration::from_secs(30));
    /// ```
    pub fn with_cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Sets the maximum storage size in megabytes
    /// 
    /// When the storage is full, new inserts will be rejected.
    /// Set to 0 for unlimited storage.
    /// 
    /// # Arguments
    /// 
    /// * `mb` - Maximum storage size in megabytes
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use memorize_core::StoreConfig;
    /// 
    /// // Allow up to 256MB of storage
    /// let config = StoreConfig::default()
    ///     .with_max_storage_mb(256);
    /// 
    /// // Unlimited storage
    /// let config = StoreConfig::default()
    ///     .with_max_storage_mb(0);
    /// ```
    pub fn with_max_storage_mb(mut self, mb: usize) -> Self {
        self.max_storage_bytes = mb * 1024 * 1024;
        self
    }

    /// Sets the maximum storage size in bytes
    /// 
    /// When the storage is full, new inserts will be rejected.
    /// Set to 0 for unlimited storage.
    pub fn with_max_storage_bytes(mut self, bytes: usize) -> Self {
        self.max_storage_bytes = bytes;
        self
    }

    /// Sets what happens when the storage limit is reached
    ///
    /// # Example
    ///
    /// ```rust
    /// use memorize_core::{EvictionPolicy, StoreConfig};
    ///
    /// // Reject writes instead of evicting when full
    /// let config = StoreConfig::default()
    ///     .with_eviction_policy(EvictionPolicy::NoEviction);
    /// ```
    pub fn with_eviction_policy(mut self, policy: EvictionPolicy) -> Self {
        self.eviction_policy = policy;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StoreConfig::default();
        assert_eq!(config.cleanup_interval, Duration::from_secs(60));
        assert_eq!(config.max_storage_bytes, 100 * 1024 * 1024); // 100MB
        assert_eq!(config.eviction_policy, EvictionPolicy::Lru);
    }

    #[test]
    fn test_custom_cleanup_interval() {
        let config = StoreConfig::default()
            .with_cleanup_interval(Duration::from_secs(30));
        assert_eq!(config.cleanup_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_builder_pattern_chaining() {
        let config = StoreConfig::new()
            .with_cleanup_interval(Duration::from_secs(120))
            .with_max_storage_mb(256);
        assert_eq!(config.cleanup_interval, Duration::from_secs(120));
        assert_eq!(config.max_storage_bytes, 256 * 1024 * 1024);
    }

    #[test]
    fn test_max_storage_mb() {
        let config = StoreConfig::default().with_max_storage_mb(50);
        assert_eq!(config.max_storage_bytes, 50 * 1024 * 1024);
    }

    #[test]
    fn test_unlimited_storage() {
        let config = StoreConfig::default().with_max_storage_mb(0);
        assert_eq!(config.max_storage_bytes, 0);
    }

    #[test]
    fn test_eviction_policy_parsing() {
        assert_eq!("lru".parse::<EvictionPolicy>(), Ok(EvictionPolicy::Lru));
        assert_eq!(" LRU ".parse::<EvictionPolicy>(), Ok(EvictionPolicy::Lru));
        assert_eq!("noeviction".parse::<EvictionPolicy>(), Ok(EvictionPolicy::NoEviction));
        assert_eq!("NoEviction".parse::<EvictionPolicy>(), Ok(EvictionPolicy::NoEviction));
        assert!("random".parse::<EvictionPolicy>().is_err());
        assert!("".parse::<EvictionPolicy>().is_err());
    }

    #[test]
    fn test_eviction_policy_display_round_trips() {
        for policy in [EvictionPolicy::Lru, EvictionPolicy::NoEviction] {
            assert_eq!(policy.to_string().parse::<EvictionPolicy>(), Ok(policy));
        }
    }
}
