use std::time::Instant;

/// Represents a stored value with its expiration time
#[derive(Debug, Clone)]
pub struct Entry {
    value: String,
    expires_at: Instant,
}

impl Entry {
    /// Creates a new entry with the given value and expiration time
    pub fn new(value: String, expires_at: Instant) -> Self {
        Self { value, expires_at }
    }

    /// Returns the stored value
    pub fn value(&self) -> &str {
        &self.value
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
    use std::time::Duration;

    #[test]
    fn test_entry_not_expired() {
        let entry = Entry::new(
            "test_value".to_string(),
            Instant::now() + Duration::from_secs(60),
        );
        
        assert_eq!(entry.value(), "test_value");
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_entry_expired() {
        let entry = Entry::new(
            "test_value".to_string(),
            Instant::now() - Duration::from_secs(1),
        );
        
        assert!(entry.is_expired());
    }
}
