//! Client configuration options.

/// Options for configuring the Memorize client connection.
///
/// # Example
///
/// ```rust
/// use memorize_client::MemorizeClientOptions;
///
/// let options = MemorizeClientOptions::new("http://localhost:50051")
///     .with_api_key("your-secret-key");
/// ```
#[derive(Clone, Debug)]
pub struct MemorizeClientOptions {
    /// The server URL (e.g., "http://localhost:50051")
    pub url: String,

    /// Optional API key for authentication
    pub api_key: Option<String>,
}

impl MemorizeClientOptions {
    /// Create new options with the given server URL.
    ///
    /// # Arguments
    /// * `url` - The Memorize server URL (e.g., "http://localhost:50051")
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            api_key: None,
        }
    }

    /// Set the API key for authentication.
    ///
    /// # Arguments
    /// * `api_key` - The API key to use for all requests
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Create options from environment variables.
    ///
    /// Reads:
    /// - `MEMORIZE_SERVER_URL` - Server URL (defaults to "http://127.0.0.1:50051")
    /// - `MEMORIZE_API_KEY` - Optional API key
    pub fn from_env() -> Self {
        let url = std::env::var("MEMORIZE_SERVER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
        let api_key = std::env::var("MEMORIZE_API_KEY").ok();

        Self { url, api_key }
    }
}

impl Default for MemorizeClientOptions {
    fn default() -> Self {
        Self::new("http://127.0.0.1:50051")
    }
}
