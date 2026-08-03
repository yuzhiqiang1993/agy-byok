use super::ProxyServer;
use crate::domain::{ErrorCategory, ProxyError};
use reqwest::Client;
use std::time::Duration;

pub(super) const MAX_PROVIDER_HTTP_CLIENTS: usize = 8;

impl ProxyServer {
    pub(crate) fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub(super) fn provider_http_client(
        &self,
        connect_timeout_ms: u64,
    ) -> Result<Client, ProxyError> {
        let mut clients = self.provider_http_clients.lock().map_err(|_| {
            ProxyError::new(
                ErrorCategory::Internal,
                "Provider HTTP client cache lock is poisoned",
                500,
            )
        })?;
        if let Some(client) = clients.get(&connect_timeout_ms) {
            return Ok(client.clone());
        }
        if clients.len() >= MAX_PROVIDER_HTTP_CLIENTS {
            clients.clear();
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_millis(connect_timeout_ms))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| {
                ProxyError::new(
                    ErrorCategory::Internal,
                    format!("Failed to create Provider HTTP client: {error}"),
                    500,
                )
            })?;
        clients.insert(connect_timeout_ms, client.clone());
        Ok(client)
    }
}
