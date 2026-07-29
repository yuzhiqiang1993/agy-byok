use super::activity::{ActivityItem, ActivityLog};
use super::auth::AuthManager;
use super::streaming::StreamPipe;
use crate::antigravity::{
    AntigravityModelDescriptor, AntigravityResponseEncoder, AntigravityStreamEncoder,
};
use crate::domain::{ErrorCategory, NeutralChatRequest, ProxyError};
use crate::providers::{get_adapter, ProviderAdapter};
use crate::routing::{ResolvedRoute, RouteTable};
use crate::storage::ConfigStore;
use reqwest::{Client, Response};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct ProxyServer {
    config_store: ConfigStore,
    activity_log: Arc<ActivityLog>,
    auth_manager: AuthManager,
    http_client: Client,
    port: u16,
}

impl ProxyServer {
    pub fn new(config_store: ConfigStore, port: u16) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            config_store,
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

    pub fn is_custom_model_id(&self, model_id: &str) -> bool {
        self.config_store
            .get_config()
            .virtual_models
            .iter()
            .any(|model| {
                model.enabled
                    && (model.id == model_id || model.effective_host_model_id() == model_id)
            })
    }

    pub(crate) fn http_client(&self) -> &Client {
        &self.http_client
    }

    /// 处理单个中立聊天请求，包含 Adapter 转译、网络发送与备用路由降级
    pub async fn handle_chat_request(
        &self,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        if request.stream {
            let mut encoded_stream = String::new();
            self.handle_chat_stream(request, |frame| {
                encoded_stream.push_str(&frame);
                Ok(())
            })
            .await?;
            return Ok(encoded_stream);
        }

        let start_time = Instant::now();
        let config = self.config_store.get_config();

        let route = match RouteTable::resolve(&config, request) {
            Ok(route) => route,
            Err(error) => {
                self.record_activity(
                    &request.virtual_model_id,
                    "unknown",
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", error.category)),
                );
                return Err(error);
            }
        };

        match self.execute_route(&route, request).await {
            Ok(response) => {
                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    200,
                    start_time.elapsed().as_millis() as u64,
                    None,
                );
                Ok(response)
            }
            Err(error) => {
                if error.is_retryable_for_fallback() {
                    if let Ok(Some(fallback_route)) = RouteTable::resolve_fallback(&config, &route)
                    {
                        tracing::info!(
                            "Primary route {} failed with {:?}, attempting fallback to {}",
                            route.virtual_model.id,
                            error.category,
                            fallback_route.virtual_model.id
                        );

                        if let Ok(fallback_response) =
                            self.execute_route(&fallback_route, request).await
                        {
                            self.record_activity(
                                &fallback_route.virtual_model.id,
                                &fallback_route.provider.id,
                                200,
                                start_time.elapsed().as_millis() as u64,
                                None,
                            );
                            return Ok(fallback_response);
                        }
                    }
                }

                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", error.category)),
                );
                Err(error)
            }
        }
    }

    pub async fn handle_chat_stream<F>(
        &self,
        request: &NeutralChatRequest,
        mut on_frame: F,
    ) -> Result<(), ProxyError>
    where
        F: FnMut(String) -> Result<(), ProxyError> + Send,
    {
        if !request.stream {
            return Err(ProxyError::new(
                ErrorCategory::InvalidRequest,
                "Streaming handler requires request.stream = true",
                400,
            ));
        }

        let start_time = Instant::now();
        let config = self.config_store.get_config();
        let route = match RouteTable::resolve(&config, request) {
            Ok(route) => route,
            Err(error) => {
                self.record_activity(
                    &request.virtual_model_id,
                    "unknown",
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", error.category)),
                );
                return Err(error);
            }
        };

        let mut emitted_frame = false;
        match self
            .execute_stream_route(&route, request, &mut on_frame, &mut emitted_frame)
            .await
        {
            Ok(()) => {
                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    200,
                    start_time.elapsed().as_millis() as u64,
                    None,
                );
                Ok(())
            }
            Err(primary_error) => {
                if !emitted_frame && primary_error.is_retryable_for_fallback() {
                    if let Ok(Some(fallback_route)) = RouteTable::resolve_fallback(&config, &route)
                    {
                        tracing::info!(
                            "Primary stream route {} failed with {:?}, attempting fallback to {}",
                            route.virtual_model.id,
                            primary_error.category,
                            fallback_route.virtual_model.id
                        );

                        if self
                            .execute_stream_route(
                                &fallback_route,
                                request,
                                &mut on_frame,
                                &mut emitted_frame,
                            )
                            .await
                            .is_ok()
                        {
                            self.record_activity(
                                &fallback_route.virtual_model.id,
                                &fallback_route.provider.id,
                                200,
                                start_time.elapsed().as_millis() as u64,
                                None,
                            );
                            return Ok(());
                        }
                    }
                }

                self.record_activity(
                    &route.virtual_model.id,
                    &route.provider.id,
                    primary_error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    Some(format!("{:?}", primary_error.category)),
                );
                Err(primary_error)
            }
        }
    }

    async fn execute_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to read upstream response body: {error}"),
                500,
            )
        })?;
        let neutral_response = adapter.parse_response(status, &body, &route.upstream_model)?;
        Ok(AntigravityResponseEncoder::encode_response(
            &neutral_response,
        ))
    }

    async fn execute_stream_route<F>(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        on_frame: &mut F,
        emitted_frame: &mut bool,
    ) -> Result<(), ProxyError>
    where
        F: FnMut(String) -> Result<(), ProxyError> + Send,
    {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.map_err(|error| {
                ProxyError::new(
                    ErrorCategory::Internal,
                    format!("Failed to read upstream error body: {error}"),
                    500,
                )
            })?;
            return match adapter.parse_response(status, &body, &route.upstream_model) {
                Err(error) => Err(error),
                Ok(_) => Err(ProxyError::new(
                    ErrorCategory::UpstreamServerError,
                    format!("Unexpected successful parse for upstream status {status}"),
                    502,
                )),
            };
        }

        let mut provider_decoder = adapter.create_stream_decoder(&route.upstream_model);
        let mut response_encoder = AntigravityStreamEncoder::new();
        StreamPipe::process_stream(
            response,
            route.provider.stream_idle_timeout_ms,
            provider_decoder.as_mut(),
            |event| {
                for frame in response_encoder.encode_event(&event)? {
                    *emitted_frame = true;
                    on_frame(frame)?;
                }
                Ok(())
            },
        )
        .await
    }

    async fn send_upstream(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(Arc<dyn ProviderAdapter>, Response), ProxyError> {
        let adapter = get_adapter(&route.provider.protocol);
        let payload = adapter.build_request_payload(route, request)?;
        let headers = adapter.build_headers(&route.provider)?;

        let mut request_builder = self
            .http_client
            .post(&route.provider.generate_endpoint)
            .json(&payload);
        for (name, value) in headers {
            request_builder = request_builder.header(name, value);
        }

        let timeout_ms = if route.provider.request_timeout_ms > 0 {
            route.provider.request_timeout_ms
        } else {
            60000
        };
        let response = request_builder
            .timeout(Duration::from_millis(timeout_ms))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProxyError::new(
                        ErrorCategory::Timeout,
                        format!("Upstream timeout: {error}"),
                        504,
                    )
                } else {
                    ProxyError::new(
                        ErrorCategory::ConnectionFailed,
                        format!("Failed to connect to upstream: {error}"),
                        502,
                    )
                }
            })?;

        Ok((adapter, response))
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
            .map(|duration| duration.as_millis() as u64)
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
