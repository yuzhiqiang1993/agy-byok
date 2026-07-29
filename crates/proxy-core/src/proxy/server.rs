use super::activity::{ActivityItem, ActivityLog};
use super::auth::AuthManager;
use crate::antigravity::{AntigravityModelDescriptor, AntigravityResponseEncoder};
use crate::domain::{ErrorCategory, NeutralChatRequest, ProxyError};
use crate::providers::get_adapter;
use crate::routing::{ResolvedRoute, RouteTable};
use crate::storage::{ConfigStore, KeyStore};
use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct ProxyServer {
    config_store: ConfigStore,
    key_store: Arc<dyn KeyStore>,
    activity_log: Arc<ActivityLog>,
    auth_manager: AuthManager,
    http_client: Client,
    port: u16,
}

impl ProxyServer {
    pub fn new(config_store: ConfigStore, key_store: Arc<dyn KeyStore>, port: u16) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            config_store,
            key_store,
            activity_log: Arc::new(ActivityLog::new()),
            auth_manager: AuthManager::new(),
            http_client,
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

    /// 处理单个中立聊天请求，包含 Key 读取、Adapter 转译、网络发送与备用路由降级
    pub async fn handle_chat_request(
        &self,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        let start_time = Instant::now();
        let config = self.config_store.get_config();

        let route = match RouteTable::resolve(&config, request) {
            Ok(r) => r,
            Err(e) => {
                self.record_activity(
                    &request.virtual_model_id,
                    "unknown",
                    e.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", e.category)),
                );
                return Err(e);
            }
        };

        // 尝试首次主路由执行
        match self.execute_route(&route, request).await {
            Ok(resp_str) => {
                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    200,
                    start_time.elapsed().as_millis() as u64,
                    None,
                );
                Ok(resp_str)
            }
            Err(err) => {
                // 如果开启了备用路由，且错误符合可降级条件，尝试降级
                if err.is_retryable_for_fallback() {
                    if let Ok(Some(fallback_route)) = RouteTable::resolve_fallback(&config, &route)
                    {
                        tracing::info!(
                            "Primary route {} failed with {:?}, attempting fallback to {}",
                            route.virtual_model.id,
                            err.category,
                            fallback_route.virtual_model.id
                        );

                        if let Ok(fallback_resp) =
                            self.execute_route(&fallback_route, request).await
                        {
                            self.record_activity(
                                &fallback_route.virtual_model.id,
                                &fallback_route.provider.id,
                                200,
                                start_time.elapsed().as_millis() as u64,
                                None,
                            );
                            return Ok(fallback_resp);
                        }
                    }
                }

                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    err.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", err.category)),
                );
                Err(err)
            }
        }
    }

    async fn execute_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        let adapter = get_adapter(&route.provider.protocol);
        let payload = adapter.build_request_payload(route, request)?;

        // 从 KeyStore 安全取回秘钥
        let api_key = self
            .key_store
            .get_secret(&route.provider.api_key_ref)
            .await
            .unwrap_or_default();

        let headers_map = adapter.build_headers(&route.provider, &api_key)?;

        let mut req_builder = self
            .http_client
            .post(&route.provider.generate_endpoint)
            .json(&payload);

        for (k, v) in headers_map {
            req_builder = req_builder.header(k, v);
        }

        let timeout_ms = if route.provider.request_timeout_ms > 0 {
            route.provider.request_timeout_ms
        } else {
            60000
        };

        let response = req_builder
            .timeout(Duration::from_millis(timeout_ms))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProxyError::new(
                        ErrorCategory::Timeout,
                        format!("Upstream timeout: {}", e),
                        504,
                    )
                } else {
                    ProxyError::new(
                        ErrorCategory::ConnectionFailed,
                        format!("Failed to connect to upstream: {}", e),
                        502,
                    )
                }
            })?;

        let status = response.status().as_u16();
        let body_text = response.text().await.map_err(|e| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to read upstream response body: {}", e),
                500,
            )
        })?;

        let neutral_resp = adapter.parse_response(status, &body_text, &route.upstream_model)?;
        let encoded_resp = AntigravityResponseEncoder::encode_response(&neutral_resp);
        Ok(encoded_resp)
    }

    fn record_activity(
        &self,
        vm_id: &str,
        provider_id: &str,
        status: u16,
        duration_ms: u64,
        error_category: Option<String>,
    ) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms: now_ms,
            virtual_model_id: vm_id.to_string(),
            provider_id: provider_id.to_string(),
            status_code: status,
            duration_ms,
            error_category,
            prompt_tokens: None,
            completion_tokens: None,
        });
    }

    /// 注入并融合包含自定义虚拟模型的模型列表描述 JSON
    pub fn handle_model_list(&self, mut base_json: serde_json::Value) -> serde_json::Value {
        let config = self.config_store.get_config();
        AntigravityModelDescriptor::inject_into_model_list(
            &mut base_json,
            &config.virtual_models,
            &config.upstream_models,
        );
        base_json
    }
}
