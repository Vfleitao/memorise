use std::time::Duration;

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
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            cleanup_interval: Duration::from_secs(60),
            max_storage_bytes: 100 * 1024 * 1024, // 100MB default
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StoreConfig::default();
        assert_eq!(config.cleanup_interval, Duration::from_secs(60));
        assert_eq!(config.max_storage_bytes, 100 * 1024 * 1024); // 100MB
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
}
