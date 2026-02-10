mod auth;
mod service;

use auth::auth_interceptor;
use memorize_core::{Store, StoreConfig};
use memorize_proto::memorize_server::MemorizeServer;
use service::MemorizeService;
use std::time::Duration;
use tonic::transport::Server;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memorize_server=info,tonic=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Configuration from environment variables
    let host = std::env::var("MEMORIZE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("MEMORIZE_PORT").unwrap_or_else(|_| "50051".to_string());
    let cleanup_interval: u64 = std::env::var("MEMORIZE_CLEANUP_INTERVAL")
        .unwrap_or_else(|_| "60".to_string())
        .parse()
        .unwrap_or(60);
    let max_storage_mb: usize = std::env::var("MEMORIZE_MAX_STORAGE_MB")
        .unwrap_or_else(|_| "100".to_string())
        .parse()
        .unwrap_or(100);
    let api_key = std::env::var("MEMORIZE_API_KEY").ok();

    let addr = format!("{}:{}", host, port).parse()?;

    // Create the store with configuration
    let config = StoreConfig::default()
        .with_cleanup_interval(Duration::from_secs(cleanup_interval))
        .with_max_storage_mb(max_storage_mb);
    let store = Store::with_config(config);

    let auth_enabled = api_key.is_some();
    let service = MemorizeService::new(store, auth_enabled);

    tracing::info!("🚀 Memorize gRPC server listening on {}", addr);
    tracing::info!("   Cleanup interval: {}s", cleanup_interval);
    if max_storage_mb == 0 {
        tracing::info!("   Max storage: unlimited");
    } else {
        tracing::info!("   Max storage: {} MB", max_storage_mb);
    }
    if api_key.is_some() {
        tracing::info!("   Authentication: enabled (API key required)");
    } else {
        tracing::warn!("   Authentication: disabled (MEMORIZE_API_KEY not set) - this is not recommended for production");
    }

    let service_with_auth = MemorizeServer::with_interceptor(service, auth_interceptor(api_key));

    Server::builder()
        .tcp_nodelay(true)
        .http2_keepalive_interval(Some(Duration::from_secs(60)))
        .add_service(service_with_auth)
        .serve(addr)
        .await?;

    Ok(())
}
