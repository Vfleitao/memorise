//! Simple API key authentication interceptor
//!
//! Validates requests against the `MEMORIZE_API_KEY` environment variable.
//! If the environment variable is not set, authentication is disabled.

use subtle::{Choice, ConstantTimeEq};
use tonic::{Request, Status};

/// The metadata key for the API key header
pub const API_KEY_HEADER: &str = "x-api-key";

/// Performs a constant-time comparison of two strings to prevent timing attacks.
/// Both length and content are compared in constant time to avoid leaking
/// the expected key's length through timing side-channels.
fn constant_time_compare(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    let max_len = a_bytes.len().max(b_bytes.len());
    if max_len == 0 {
        return true;
    }

    // Pad both to equal length so the comparison always processes
    // the same number of bytes regardless of input lengths.
    let mut a_padded = vec![0u8; max_len];
    let mut b_padded = vec![0u8; max_len];
    a_padded[..a_bytes.len()].copy_from_slice(a_bytes);
    b_padded[..b_bytes.len()].copy_from_slice(b_bytes);

    let content_eq = a_padded.ct_eq(&b_padded);
    let length_eq = Choice::from((a_bytes.len() == b_bytes.len()) as u8);

    (content_eq & length_eq).into()
}

/// Interceptor that validates the API key from request metadata
pub fn auth_interceptor(
    api_key: Option<String>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| {
        // If no API key is configured, allow all requests
        let Some(expected_key) = &api_key else {
            return Ok(req);
        };

        // Check for the API key in metadata
        match req.metadata().get(API_KEY_HEADER) {
            Some(provided_key) => {
                let provided_key = provided_key
                    .to_str()
                    .map_err(|_| Status::unauthenticated("Invalid API key format"))?;

                if constant_time_compare(provided_key, expected_key) {
                    Ok(req)
                } else {
                    tracing::warn!("Invalid API key provided");
                    Err(Status::unauthenticated("Invalid API key"))
                }
            }
            None => {
                tracing::warn!("Missing API key in request");
                Err(Status::unauthenticated("Missing API key"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("secret123", "secret123"));
        assert!(!constant_time_compare("secret123", "secret124"));
        assert!(!constant_time_compare("secret123", "secret12"));
        assert!(!constant_time_compare("short", "muchlonger"));
        assert!(constant_time_compare("", ""));
    }

    #[test]
    fn test_constant_time_compare_one_empty() {
        // Verify that comparing a non-empty string against empty returns false
        // (exercises the padding path where one side is entirely padding)
        assert!(!constant_time_compare("secret", ""));
        assert!(!constant_time_compare("", "secret"));
    }

    #[test]
    fn test_constant_time_compare_same_prefix_different_length() {
        // Strings that share a prefix but differ in length.
        // The old early-return on length would short-circuit here;
        // the new code must still reject in constant time.
        assert!(!constant_time_compare("secret123", "secret1234"));
        assert!(!constant_time_compare("secret1234", "secret123"));
    }

    #[test]
    fn test_no_api_key_configured_allows_all() {
        let interceptor = auth_interceptor(None);
        let req = Request::new(());
        assert!(interceptor(req).is_ok());
    }

    #[test]
    fn test_valid_api_key() {
        let interceptor = auth_interceptor(Some("secret123".to_string()));
        let mut req = Request::new(());
        req.metadata_mut()
            .insert(API_KEY_HEADER, "secret123".parse().unwrap());
        assert!(interceptor(req).is_ok());
    }

    #[test]
    fn test_invalid_api_key() {
        let interceptor = auth_interceptor(Some("secret123".to_string()));
        let mut req = Request::new(());
        req.metadata_mut()
            .insert(API_KEY_HEADER, "wrong-key".parse().unwrap());
        assert!(interceptor(req).is_err());
    }

    #[test]
    fn test_missing_api_key() {
        let interceptor = auth_interceptor(Some("secret123".to_string()));
        let req = Request::new(());
        assert!(interceptor(req).is_err());
    }
}
