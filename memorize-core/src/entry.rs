use bytes::Bytes;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Represents a stored value with its expiration time and last access time
#[derive(Debug)]
pub struct Entry {
    value: Bytes,
    expires_at: Instant,
    /// Last access time in whole seconds since the store was created.
    /// Used to pick eviction victims (approximate LRU).
    last_access: AtomicU32,
}

impl Entry {
    /// Creates a new entry with the given value, expiration time and access time
    pub fn new(value: Bytes, expires_at: Instant, last_access: u32) -> Self {
        Self {
            value,
            expires_at,
            last_access: AtomicU32::new(last_access),
        }
    }

    /// Returns the stored value
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Returns a shared handle to the stored value (reference-count bump, no copy)
    pub fn value_shared(&self) -> Bytes {
        self.value.clone()
    }

    /// Returns the expiration time
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    /// Checks if this entry has expired
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Instant::now())
    }

    /// Checks if this entry has expired as of `now`
    ///
    /// Lets scans over many entries read the clock once instead of per entry.
    pub fn is_expired_at(&self, now: Instant) -> bool {
        now >= self.expires_at
    }

    /// Returns the last access time in seconds since the store was created
    pub fn last_access(&self) -> u32 {
        self.last_access.load(Ordering::Relaxed)
    }

    /// Records an access at `now` (seconds since the store was created)
    ///
    /// Only writes when the second has changed, so hot keys read by many
    /// threads don't keep invalidating each other's cache line.
    pub fn touch(&self, now: u32) {
        if self.last_access.load(Ordering::Relaxed) != now {
            self.last_access.store(now, Ordering::Relaxed);
        }
    }
}

impl Clone for Entry {
    fn clone(&self) -> Self {
        Self::new(self.value_shared(), self.expires_at, self.last_access())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_entry_not_expired() {
        let entry = Entry::new(
            Bytes::from_static(b"test_value"),
            Instant::now() + Duration::from_secs(60),
            0,
        );

        assert_eq!(entry.value(), b"test_value");
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_entry_expired() {
        let entry = Entry::new(
            Bytes::from_static(b"test_value"),
            Instant::now() - Duration::from_secs(1),
            0,
        );

        assert!(entry.is_expired());
    }

    #[test]
    fn test_is_expired_at() {
        let now = Instant::now();
        let entry = Entry::new(Bytes::from_static(b"v"), now + Duration::from_secs(10), 0);

        assert!(!entry.is_expired_at(now));
        assert!(entry.is_expired_at(now + Duration::from_secs(10)));
        assert!(entry.is_expired_at(now + Duration::from_secs(11)));
    }

    #[test]
    fn test_touch_updates_last_access() {
        let entry = Entry::new(Bytes::from_static(b"v"), Instant::now() + Duration::from_secs(60), 3);
        assert_eq!(entry.last_access(), 3);

        entry.touch(7);
        assert_eq!(entry.last_access(), 7);

        // Touching with the same second is a no-op
        entry.touch(7);
        assert_eq!(entry.last_access(), 7);
    }

    #[test]
    fn test_clone_copies_last_access() {
        let entry = Entry::new(Bytes::from(b"v".to_vec()), Instant::now() + Duration::from_secs(60), 5);
        let cloned = entry.clone();

        assert_eq!(cloned.last_access(), 5);
        assert_eq!(cloned.expires_at(), entry.expires_at());
        assert_eq!(cloned.value_shared().as_ptr(), entry.value_shared().as_ptr());
    }

    #[test]
    fn test_value_shared_does_not_copy() {
        let entry = Entry::new(
            Bytes::from(b"shared_value".to_vec()),
            Instant::now() + Duration::from_secs(60),
            0,
        );

        let shared1 = entry.value_shared();
        let shared2 = entry.value_shared();
        // Both should point to the same allocation
        assert_eq!(shared1.as_ptr(), shared2.as_ptr());
        assert_eq!(&shared1[..], b"shared_value");
    }
}
