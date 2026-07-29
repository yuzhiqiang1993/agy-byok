use agy_byok::proxy::ProxyServer;
use agy_byok::storage::{AppConfig, ConfigStore, KeychainStore};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting AGY BYOK Proxy Core v0.1.0...");

    let config_store = ConfigStore::in_memory(AppConfig::default());
    let key_store = Arc::new(KeychainStore::new());

    let server = ProxyServer::new(config_store, key_store, 50999);
    tracing::info!("AGY BYOK Proxy initialized on port {}", server.port());

    // 保持主线程运行
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down AGY BYOK Proxy...");

    Ok(())
}
