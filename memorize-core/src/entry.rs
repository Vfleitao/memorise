use std::sync::Arc;
use std::time::Instant;

/// Represents a stored value with its expiration time
#[derive(Debug, Clone)]
pub struct Entry {
    value: Arc<str>,
    expires_at: Instant,
}

impl Entry {
    /// Creates a new entry with the given value and expiration time
    pub fn new(value: Arc<str>, expires_at: Instant) -> Self {
        Self { value, expires_at }
    }

    /// Returns the stored value as a string slice
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns a shared reference to the stored value (zero-cost clone)
    pub fn value_shared(&self) -> Arc<str> {
        Arc::clone(&self.value)
    }

    /// Returns the expiration time
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    /// Checks if this entry has expired
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_entry_not_expired() {
        let entry = Entry::new(
            Arc::from("test_value"),
            Instant::now() + Duration::from_secs(60),
        );

        assert_eq!(entry.value(), "test_value");
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_entry_expired() {
        let entry = Entry::new(
            Arc::from("test_value"),
            Instant::now() - Duration::from_secs(1),
        );

        assert!(entry.is_expired());
    }

    #[test]
    fn test_value_shared_returns_arc() {
        let entry = Entry::new(
            Arc::from("shared_value"),
            Instant::now() + Duration::from_secs(60),
        );

        let shared1 = entry.value_shared();
        let shared2 = entry.value_shared();
        // Both should point to the same allocation
        assert!(Arc::ptr_eq(&shared1, &shared2));
        assert_eq!(&*shared1, "shared_value");
    }
}
