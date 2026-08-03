mod activity_recorder;
mod client_pool;
mod execution;
mod fallback;
mod model_catalog;

use crate::storage::ConfigStore;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::activity::ActivityLog;
use super::auth::AuthManager;

pub(crate) use execution::EncodedFrameSink;

#[cfg(test)]
use crate::domain::{ErrorCategory, ProxyError, ReasoningLevel};
#[cfg(test)]
use client_pool::MAX_PROVIDER_HTTP_CLIENTS;
#[cfg(test)]
use execution::{
    effective_provider_connect_timeout_ms, effective_provider_request_timeout_ms,
    DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS, DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS,
};
#[cfg(test)]
use model_catalog::configured_model_display_name;

pub struct ProxyServer {
    config_store: ConfigStore,
    activity_log: Arc<ActivityLog>,
    auth_manager: AuthManager,
    http_client: Client,
    provider_http_clients: Mutex<HashMap<u64, Client>>,
    port: u16,
}

impl ProxyServer {
    pub fn new(config_store: ConfigStore, port: u16) -> Self {
        Self::with_activity_log(config_store, port, Arc::new(ActivityLog::new()))
    }

    pub fn with_activity_log(
        config_store: ConfigStore,
        port: u16,
        activity_log: Arc<ActivityLog>,
    ) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            config_store,
            activity_log,
            auth_manager: AuthManager::new(),
            http_client,
            provider_http_clients: Mutex::new(HashMap::new()),
            port,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn auth_manager(&self) -> &AuthManager {
        &self.auth_manager
    }

    pub fn activity_log(&self) -> Arc<ActivityLog> {
        self.activity_log.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_suffix_is_added_once_to_catalog_display_name() {
        assert_eq!(
            configured_model_display_name(
                "GPT Test",
                Some(ReasoningLevel::High),
                "Provider A",
                true
            ),
            "GPT Test high(Provider A)"
        );
        assert_eq!(
            configured_model_display_name(
                "GPT Test high(Provider A)",
                Some(ReasoningLevel::High),
                "Provider A",
                true
            ),
            "GPT Test high(Provider A)"
        );
        assert_eq!(
            configured_model_display_name(
                "GPT Test low(Provider A)",
                Some(ReasoningLevel::Max),
                "Provider A",
                true
            ),
            "GPT Test max(Provider A)"
        );
        assert_eq!(
            configured_model_display_name("GPT Test", None, "Provider A", false),
            "GPT Test(Provider A)"
        );
        assert_eq!(
            configured_model_display_name("GPT Test high(Provider A)", None, "Provider A", false),
            "GPT Test(Provider A)"
        );
    }

    #[test]
    fn upstream_error_detail_keeps_diagnostics_and_redacts_credentials() {
        let error = ProxyError::new(ErrorCategory::InvalidRequest, "rejected", 400)
            .with_upstream_body(
                r#"{"error":{"message":"Invalid schema; token sk-secret-value Bearer secret-token","type":"invalid_request_error","param":"tools[0].parameters","code":"invalid_function_parameters"}}"#,
            );

        let detail = ProxyServer::sanitized_upstream_error(&error).unwrap();

        assert!(detail.contains("Invalid schema"));
        assert!(detail.contains("param=tools[0].parameters"));
        assert!(detail.contains("code=invalid_function_parameters"));
        assert!(!detail.contains("sk-secret-value"));
        assert!(!detail.contains("secret-token"));
        assert!(detail.contains("[REDACTED]"));
    }

    #[test]
    fn provider_timeouts_use_defaults_and_cap_connect_timeout() {
        assert_eq!(
            effective_provider_request_timeout_ms(0),
            DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS
        );
        assert_eq!(effective_provider_request_timeout_ms(12_000), 12_000);
        assert_eq!(
            effective_provider_connect_timeout_ms(0, 60_000),
            DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS
        );
        assert_eq!(
            effective_provider_connect_timeout_ms(15_000, 10_000),
            10_000
        );
    }

    #[test]
    fn provider_clients_are_cached_and_bounded_by_effective_connect_timeout() {
        let server = ProxyServer::new(ConfigStore::in_memory(Default::default()), 0);

        server.provider_http_client(1_000).unwrap();
        server.provider_http_client(1_000).unwrap();
        server.provider_http_client(2_000).unwrap();
        assert_eq!(server.provider_http_clients.lock().unwrap().len(), 2);

        for timeout_ms in (3_000..=12_000).step_by(1_000) {
            server.provider_http_client(timeout_ms).unwrap();
        }
        assert!(server.provider_http_clients.lock().unwrap().len() <= MAX_PROVIDER_HTTP_CLIENTS);
    }
}
