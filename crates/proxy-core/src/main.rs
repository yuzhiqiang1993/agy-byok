use agy_byok::proxy::{HttpServerOptions, LoopbackHttpServer, ProxyServer};
use agy_byok::storage::{default_config_path, ConfigStore, KeychainStore};
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

    let config_path = default_config_path().map_err(std::io::Error::other)?;
    let config_file_exists = config_path.exists();
    let config_store = ConfigStore::load_from_file(&config_path).map_err(std::io::Error::other)?;
    if !config_file_exists {
        config_store
            .update_config(config_store.get_config())
            .map_err(std::io::Error::other)?;
    }
    let key_store = Arc::new(KeychainStore::new());

    let server = Arc::new(ProxyServer::new(config_store, key_store, 50999));
    let http_server = LoopbackHttpServer::start(server, HttpServerOptions::default()).await?;
    tracing::info!("AGY BYOK Proxy listening on {}", http_server.local_addr());

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down AGY BYOK Proxy...");
    http_server.shutdown().await?;

    Ok(())
}
