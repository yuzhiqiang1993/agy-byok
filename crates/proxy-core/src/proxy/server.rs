use super::activity::{ActivityItem, ActivityLog};
use super::auth::AuthManager;
use super::streaming::{NeutralEventSink, StreamPipe};
use crate::antigravity::{
    AntigravityModelDescriptor, AntigravityResponseEncoder, AntigravityStreamEncoder,
};
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage,
    NeutralStreamEvent, ParameterOverrides, ProviderProtocol, ProxyError,
};
use crate::providers::{get_adapter, ProviderAdapter};
use crate::routing::{ResolvedRoute, RouteTable};
use crate::storage::ConfigStore;
use async_trait::async_trait;
use reqwest::{Client, Response};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CONNECTION_TEST_TIMEOUT_MS: u64 = 15_000;

#[async_trait]
pub(crate) trait EncodedFrameSink: Send {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError>;
}

struct CallbackFrameSink<F> {
    callback: F,
}

#[async_trait]
impl<F> EncodedFrameSink for CallbackFrameSink<F>
where
    F: FnMut(String) -> Result<(), ProxyError> + Send,
{
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        (self.callback)(frame)
    }
}

struct StringFrameSink<'a> {
    buffer: &'a mut String,
}

#[async_trait]
impl EncodedFrameSink for StringFrameSink<'_> {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        self.buffer.push_str(&frame);
        Ok(())
    }
}

struct AntigravityEventSink<'a> {
    encoder: AntigravityStreamEncoder,
    frame_sink: &'a mut dyn EncodedFrameSink,
    emitted_frame: &'a mut bool,
}

#[async_trait]
impl NeutralEventSink for AntigravityEventSink<'_> {
    async fn send(&mut self, event: NeutralStreamEvent) -> Result<(), ProxyError> {
        for frame in self.encoder.encode_event(&event)? {
            *self.emitted_frame = true;
            self.frame_sink.send(frame).await?;
        }
        Ok(())
    }
}

pub struct ProxyServer {
    config_store: ConfigStore,
    activity_log: Arc<ActivityLog>,
    auth_manager: AuthManager,
    http_client: Client,
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

    pub fn record_official_generation(
        &self,
        model_id: &str,
        stream: bool,
        message_count: usize,
        tool_count: usize,
        status_code: u16,
        duration_ms: u64,
    ) {
        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms: Self::current_time_ms(),
            virtual_model_id: model_id.to_string(),
            upstream_model_id: Some(model_id.to_string()),
            provider_id: "antigravity-official".to_string(),
            provider_protocol: Some("native".to_string()),
            status_code,
            duration_ms,
            error_category: (!matches!(status_code, 200..=299))
                .then(|| "OfficialUpstream".to_string()),
            error_detail: None,
            stream,
            message_count,
            tool_count,
            used_fallback: false,
            prompt_tokens: None,
            completion_tokens: None,
        });
    }

    pub fn is_custom_model_id(&self, model_id: &str) -> bool {
        self.config_store
            .get_config()
            .virtual_models
            .iter()
            .any(|model| model.enabled && model.matches_id(model_id))
    }

    pub(crate) fn http_client(&self) -> &Client {
        &self.http_client
    }

    /// 发送最小非流式请求，验证指定模型的路由、鉴权和响应解析。
    pub async fn test_model_connection(&self, virtual_model_id: &str) -> Result<(), ProxyError> {
        let config = self.config_store.get_config();
        let request = NeutralChatRequest {
            virtual_model_id: virtual_model_id.to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Reply with OK.".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides {
                max_tokens: Some(8),
                ..ParameterOverrides::default()
            },
            extra_body: Default::default(),
        };
        let mut route = RouteTable::resolve(&config, &request)?;
        route.final_reasoning_level = None;
        route.provider.request_timeout_ms = match route.provider.request_timeout_ms {
            0 => CONNECTION_TEST_TIMEOUT_MS,
            configured => configured.min(CONNECTION_TEST_TIMEOUT_MS),
        };

        self.execute_route(&route, &request).await?;
        Ok(())
    }

    /// 处理单个中立聊天请求，包含 Adapter 转译、网络发送与备用路由降级
    pub async fn handle_chat_request(
        &self,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        if request.stream {
            let mut encoded_stream = String::new();
            let mut frame_sink = StringFrameSink {
                buffer: &mut encoded_stream,
            };
            self.handle_chat_stream_to(request, &mut frame_sink).await?;
            return Ok(encoded_stream);
        }

        let start_time = Instant::now();
        let config = self.config_store.get_config();

        let route = match RouteTable::resolve(&config, request) {
            Ok(route) => route,
            Err(error) => {
                self.record_activity(
                    None,
                    request,
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    false,
                    Some(&error),
                );
                return Err(error);
            }
        };

        match self.execute_route(&route, request).await {
            Ok(response) => {
                self.record_activity(
                    Some(&route),
                    request,
                    200,
                    start_time.elapsed().as_millis() as u64,
                    false,
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
                                Some(&fallback_route),
                                request,
                                200,
                                start_time.elapsed().as_millis() as u64,
                                true,
                                None,
                            );
                            return Ok(fallback_response);
                        }
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    false,
                    Some(&error),
                );
                Err(error)
            }
        }
    }

    pub async fn handle_chat_stream<F>(
        &self,
        request: &NeutralChatRequest,
        on_frame: F,
    ) -> Result<(), ProxyError>
    where
        F: FnMut(String) -> Result<(), ProxyError> + Send,
    {
        let mut frame_sink = CallbackFrameSink { callback: on_frame };
        self.handle_chat_stream_to(request, &mut frame_sink).await
    }

    pub(crate) async fn handle_chat_stream_to(
        &self,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
    ) -> Result<(), ProxyError> {
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
                    None,
                    request,
                    error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    false,
                    Some(&error),
                );
                return Err(error);
            }
        };

        let mut emitted_frame = false;
        match self
            .execute_stream_route(&route, request, frame_sink, &mut emitted_frame)
            .await
        {
            Ok(()) => {
                self.record_activity(
                    Some(&route),
                    request,
                    200,
                    start_time.elapsed().as_millis() as u64,
                    false,
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
                                frame_sink,
                                &mut emitted_frame,
                            )
                            .await
                            .is_ok()
                        {
                            self.record_activity(
                                Some(&fallback_route),
                                request,
                                200,
                                start_time.elapsed().as_millis() as u64,
                                true,
                                None,
                            );
                            return Ok(());
                        }
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    primary_error.status_code,
                    start_time.elapsed().as_millis() as u64,
                    false,
                    Some(&primary_error),
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

    async fn execute_stream_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
        emitted_frame: &mut bool,
    ) -> Result<(), ProxyError> {
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
        let mut event_sink = AntigravityEventSink {
            encoder: AntigravityStreamEncoder::new(),
            frame_sink,
            emitted_frame,
        };
        StreamPipe::process_stream_to(
            response,
            route.provider.stream_idle_timeout_ms,
            provider_decoder.as_mut(),
            &mut event_sink,
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
        let generate_endpoint = route
            .provider
            .generate_endpoint
            .replace("{model}", &route.upstream_model.upstream_model_id);

        let mut request_builder = self.http_client.post(generate_endpoint).json(&payload);
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
        route: Option<&ResolvedRoute>,
        request: &NeutralChatRequest,
        status_code: u16,
        duration_ms: u64,
        used_fallback: bool,
        error: Option<&ProxyError>,
    ) {
        let now_ms = Self::current_time_ms();
        let (virtual_model_id, upstream_model_id, provider_id, provider_protocol) = match route {
            Some(route) => (
                route.virtual_model.id.clone(),
                Some(route.upstream_model.upstream_model_id.clone()),
                route.provider.id.clone(),
                Some(match route.provider.protocol {
                    ProviderProtocol::Openai => "openai".to_string(),
                    ProviderProtocol::Anthropic => "anthropic".to_string(),
                    ProviderProtocol::Gemini => "gemini".to_string(),
                }),
            ),
            None => (
                request.virtual_model_id.clone(),
                None,
                "unknown".to_string(),
                None,
            ),
        };

        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp_ms: now_ms,
            virtual_model_id,
            upstream_model_id,
            provider_id,
            provider_protocol,
            status_code,
            duration_ms,
            error_category: error.map(|error| format!("{:?}", error.category)),
            error_detail: error.and_then(Self::sanitized_upstream_error),
            stream: request.stream,
            message_count: request.messages.len(),
            tool_count: request.tools.len(),
            used_fallback,
            prompt_tokens: None,
            completion_tokens: None,
        });
    }

    fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    fn sanitized_upstream_error(error: &ProxyError) -> Option<String> {
        let body = error.upstream_body.as_deref()?;
        let payload: serde_json::Value = serde_json::from_str(body).ok()?;
        let detail = payload.get("error").unwrap_or(&payload);
        if let Some(message) = detail.as_str() {
            return Some(Self::sanitize_log_text(message));
        }

        let object = detail.as_object()?;
        let fields = ["message", "type", "param", "code"];
        let parts = fields
            .into_iter()
            .filter_map(|key| {
                let value = object.get(key)?;
                let raw = match value {
                    serde_json::Value::String(value) => value.clone(),
                    serde_json::Value::Number(value) => value.to_string(),
                    serde_json::Value::Bool(value) => value.to_string(),
                    _ => return None,
                };
                Some(format!("{key}={}", Self::sanitize_log_text(&raw)))
            })
            .collect::<Vec<_>>();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    fn sanitize_log_text(value: &str) -> String {
        let mut redact_next = false;
        let mut sanitized = Vec::new();
        for token in value.split_whitespace() {
            let comparable = token
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '=')
                })
                .to_ascii_lowercase();
            if redact_next {
                sanitized.push("[REDACTED]".to_string());
                redact_next = false;
            } else if comparable == "bearer" {
                sanitized.push("Bearer".to_string());
                redact_next = true;
            } else if comparable.starts_with("sk-")
                || comparable.starts_with("api_key=")
                || comparable.starts_with("apikey=")
                || comparable.starts_with("authorization=")
            {
                sanitized.push("[REDACTED]".to_string());
            } else {
                sanitized.push(token.to_string());
            }
        }
        sanitized.join(" ").chars().take(500).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
