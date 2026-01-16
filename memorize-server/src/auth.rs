//! Simple API key authentication interceptor
//!
//! Validates requests against the `MEMORIZE_API_KEY` environment variable.
//! If the environment variable is not set, authentication is disabled.

use subtle::ConstantTimeEq;
use tonic::{Request, Status};

/// The metadata key for the API key header
pub const API_KEY_HEADER: &str = "x-api-key";

/// Performs a constant-time comparison of two strings to prevent timing attacks.
/// Returns true if the strings are equal.
fn constant_time_compare(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    
    // Length check is not constant-time, but that's acceptable since
    // the expected key length is not secret (attacker can see their own requests)
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    
    a_bytes.ct_eq(b_bytes).into()
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
