use std::time::Duration;

/// Configuration for the store's background cleanup task
/// 
/// # Example
/// 
/// ```rust
/// use memorize_core::StoreConfig;
/// use std::time::Duration;
/// 
/// let config = StoreConfig::default()
///     .with_cleanup_interval(Duration::from_secs(30));
/// ```
#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Interval between cleanup runs (default: 60 seconds)
    pub cleanup_interval: Duration,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            cleanup_interval: Duration::from_secs(60),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StoreConfig::default();
        assert_eq!(config.cleanup_interval, Duration::from_secs(60));
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
            .with_cleanup_interval(Duration::from_secs(120));
        assert_eq!(config.cleanup_interval, Duration::from_secs(120));
    }
}
