mod service;

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

    let addr = format!("{}:{}", host, port).parse()?;

    // Create the store with configuration
    let config = StoreConfig::default()
        .with_cleanup_interval(Duration::from_secs(cleanup_interval));
    let store = Store::with_config(config);

    let service = MemorizeService::new(store);

    tracing::info!("🚀 Memorize gRPC server listening on {}", addr);
    tracing::info!("   Cleanup interval: {}s", cleanup_interval);

    Server::builder()
        .add_service(MemorizeServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
