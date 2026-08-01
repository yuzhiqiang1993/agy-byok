use super::activity::{ActivityItem, ActivityLog};
use super::auth::AuthManager;
use super::streaming::{NeutralEventSink, StreamPipe};
use crate::antigravity::{
    AntigravityModelDescriptor, AntigravityResponseEncoder, AntigravityStreamEncoder,
};
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage,
    NeutralStreamEvent, ParameterOverrides, ProviderProtocol, ProxyError, ReasoningLevel,
    UsageInfo,
};
use crate::providers::{get_adapter, ProviderAdapter};
use crate::routing::{ResolvedRoute, RouteTable};
use crate::storage::ConfigStore;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use async_trait::async_trait;
use reqwest::{Client, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CONNECTION_TEST_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS: u64 = 60_000;
const MAX_PROVIDER_HTTP_CLIENTS: usize = 8;

struct ActivityOutcome<'a> {
    status_code: u16,
    duration_ms: u64,
    fallback_attempted: bool,
    fallback_succeeded: bool,
    usage: Option<&'a UsageInfo>,
    error: Option<&'a ProxyError>,
}

impl<'a> ActivityOutcome<'a> {
    fn success(duration_ms: u64, used_fallback: bool, usage: Option<&'a UsageInfo>) -> Self {
        Self {
            status_code: 200,
            duration_ms,
            fallback_attempted: used_fallback,
            fallback_succeeded: used_fallback,
            usage,
            error: None,
        }
    }

    fn failure(duration_ms: u64, fallback_attempted: bool, error: &'a ProxyError) -> Self {
        Self {
            status_code: error.status_code,
            duration_ms,
            fallback_attempted,
            fallback_succeeded: false,
            usage: None,
            error: Some(error),
        }
    }
}

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
    usage: &'a mut Option<UsageInfo>,
}

#[async_trait]
impl NeutralEventSink for AntigravityEventSink<'_> {
    async fn send(&mut self, event: NeutralStreamEvent) -> Result<(), ProxyError> {
        if let NeutralStreamEvent::UsageUpdate(usage) = &event {
            *self.usage = Some(usage.clone());
        }
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
            requested_virtual_model_id: model_id.to_string(),
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
            fallback_attempted: false,
            fallback_succeeded: false,
            prompt_tokens: None,
            completion_tokens: None,
        });
    }

    pub fn is_custom_model_id(&self, model_id: &str) -> bool {
        self.config_store
            .get_config()
            .virtual_models
            .iter()
            .any(|model| model.matches_id(model_id))
            || model_id.starts_with("custom-")
            || model_id
                .strip_prefix("MODEL_PLACEHOLDER_M")
                .and_then(|value| value.parse::<u16>().ok())
                .is_some_and(|value| (400..600).contains(&value))
    }

    pub(crate) fn http_client(&self) -> &Client {
        &self.http_client
    }

    fn provider_http_client(&self, connect_timeout_ms: u64) -> Result<Client, ProxyError> {
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
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &error,
                    ),
                );
                return Err(error);
            }
        };

        match self.execute_route(&route, request).await {
            Ok((response, usage)) => {
                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::success(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        usage.as_ref(),
                    ),
                );
                Ok(response)
            }
            Err(primary_error) => {
                if primary_error.is_retryable_for_fallback() {
                    match RouteTable::resolve_fallback(&config, &route, request) {
                        Ok(Some(fallback_route)) => {
                            tracing::info!(
                                "Primary route {} failed with {:?}, attempting fallback to {}",
                                route.virtual_model.id,
                                primary_error.category,
                                fallback_route.virtual_model.id
                            );

                            match self.execute_route(&fallback_route, request).await {
                                Ok((fallback_response, usage)) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::success(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            usage.as_ref(),
                                        ),
                                    );
                                    return Ok(fallback_response);
                                }
                                Err(fallback_error) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::failure(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            &fallback_error,
                                        ),
                                    );
                                    return Err(fallback_error);
                                }
                            }
                        }
                        Err(fallback_error) => {
                            self.record_activity(
                                Some(&route),
                                request,
                                ActivityOutcome::failure(
                                    start_time.elapsed().as_millis() as u64,
                                    true,
                                    &fallback_error,
                                ),
                            );
                            return Err(fallback_error);
                        }
                        Ok(None) => {}
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &primary_error,
                    ),
                );
                Err(primary_error)
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
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &error,
                    ),
                );
                return Err(error);
            }
        };

        let mut emitted_frame = false;
        match self
            .execute_stream_route(&route, request, frame_sink, &mut emitted_frame)
            .await
        {
            Ok(usage) => {
                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::success(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        usage.as_ref(),
                    ),
                );
                Ok(())
            }
            Err(primary_error) => {
                if !emitted_frame && primary_error.is_retryable_for_fallback() {
                    match RouteTable::resolve_fallback(&config, &route, request) {
                        Ok(Some(fallback_route)) => {
                            tracing::info!(
                                "Primary stream route {} failed with {:?}, attempting fallback to {}",
                                route.virtual_model.id,
                                primary_error.category,
                                fallback_route.virtual_model.id
                            );

                            match self
                                .execute_stream_route(
                                    &fallback_route,
                                    request,
                                    frame_sink,
                                    &mut emitted_frame,
                                )
                                .await
                            {
                                Ok(usage) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::success(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            usage.as_ref(),
                                        ),
                                    );
                                    return Ok(());
                                }
                                Err(fallback_error) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::failure(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            &fallback_error,
                                        ),
                                    );
                                    return Err(fallback_error);
                                }
                            }
                        }
                        Err(fallback_error) => {
                            self.record_activity(
                                Some(&route),
                                request,
                                ActivityOutcome::failure(
                                    start_time.elapsed().as_millis() as u64,
                                    true,
                                    &fallback_error,
                                ),
                            );
                            return Err(fallback_error);
                        }
                        Ok(None) => {}
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &primary_error,
                    ),
                );
                Err(primary_error)
            }
        }
    }

    async fn execute_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(String, Option<UsageInfo>), ProxyError> {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        let buffered =
            read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES)
                .await
                .map_err(|error| {
                    ProxyError::new(
                        ErrorCategory::Internal,
                        format!("Failed to read upstream response body: {error}"),
                        500,
                    )
                })?;
        let truncated = buffered.is_truncated();
        let body = buffered.into_text();
        if truncated && status < 400 {
            return Err(upstream_body_too_large_error());
        }
        let body = if truncated {
            format!("{body}\n[upstream error body exceeded the buffered response limit]")
        } else {
            body
        };
        let neutral_response = adapter.parse_response(status, &body, &route.upstream_model)?;
        let usage = neutral_response.usage.clone();
        Ok((
            AntigravityResponseEncoder::encode_response(&neutral_response),
            usage,
        ))
    }

    async fn execute_stream_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
        emitted_frame: &mut bool,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        if status >= 400 {
            let buffered =
                read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES)
                    .await
                    .map_err(|error| {
                        ProxyError::new(
                            ErrorCategory::Internal,
                            format!("Failed to read upstream error body: {error}"),
                            500,
                        )
                    })?;
            let truncated = buffered.is_truncated();
            let body = buffered.into_text();
            let body = if truncated {
                format!("{body}\n[upstream error body exceeded the buffered response limit]")
            } else {
                body
            };
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
        let mut usage = None;
        {
            let mut event_sink = AntigravityEventSink {
                encoder: AntigravityStreamEncoder::new(),
                frame_sink,
                emitted_frame,
                usage: &mut usage,
            };
            StreamPipe::process_stream_to(
                response,
                route.provider.stream_idle_timeout_ms,
                provider_decoder.as_mut(),
                &mut event_sink,
            )
            .await?;
        }
        Ok(usage)
    }

    async fn send_upstream(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(Arc<dyn ProviderAdapter>, Response), ProxyError> {
        let adapter = get_adapter(&route.provider.protocol);
        let payload = adapter.build_request_payload(route, request)?;
        let headers = adapter.build_headers(&route.provider)?;
        let generate_endpoint = adapter.build_generate_endpoint(
            &route.provider,
            &route.upstream_model,
            request.stream,
        )?;
        let request_timeout_ms =
            effective_provider_request_timeout_ms(route.provider.request_timeout_ms);
        let connect_timeout_ms = effective_provider_connect_timeout_ms(
            route.provider.connect_timeout_ms,
            request_timeout_ms,
        );
        let client = self.provider_http_client(connect_timeout_ms)?;

        let mut request_builder = client.post(generate_endpoint).json(&payload);
        for (name, value) in headers {
            request_builder = request_builder.header(name, value);
        }

        let response = request_builder
            .timeout(Duration::from_millis(request_timeout_ms))
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
        outcome: ActivityOutcome<'_>,
    ) {
        let now_ms = Self::current_time_ms();
        let (virtual_model_id, upstream_model_id, provider_id, provider_protocol) = match route {
            Some(route) => (
                route.virtual_model.id.clone(),
                Some(route.upstream_model.upstream_model_id.clone()),
                route.provider.id.clone(),
                Some(match route.provider.protocol {
                    ProviderProtocol::OpenaiChatCompletions => {
                        "openai_chat_completions".to_string()
                    }
                    ProviderProtocol::AnthropicMessages => "anthropic_messages".to_string(),
                    ProviderProtocol::GeminiGenerateContent => {
                        "gemini_generate_content".to_string()
                    }
                    ProviderProtocol::OpenaiResponses => "openai_responses".to_string(),
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
            requested_virtual_model_id: request.virtual_model_id.clone(),
            virtual_model_id,
            upstream_model_id,
            provider_id,
            provider_protocol,
            status_code: outcome.status_code,
            duration_ms: outcome.duration_ms,
            error_category: outcome.error.map(|error| format!("{:?}", error.category)),
            error_detail: outcome.error.map(|error| {
                Self::sanitized_upstream_error(error)
                    .unwrap_or_else(|| Self::sanitize_log_text(&error.message))
            }),
            stream: request.stream,
            message_count: request.messages.len(),
            tool_count: request.tools.len(),
            used_fallback: outcome.fallback_succeeded,
            fallback_attempted: outcome.fallback_attempted,
            fallback_succeeded: outcome.fallback_succeeded,
            prompt_tokens: outcome.usage.map(|usage| usage.prompt_tokens),
            completion_tokens: outcome.usage.map(|usage| usage.completion_tokens),
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
        let catalog_virtual_models = config
            .virtual_models
            .iter()
            .filter(|virtual_model| {
                if !virtual_model.enabled {
                    return false;
                }
                let Some(upstream_model) = config
                    .upstream_models
                    .iter()
                    .find(|upstream| upstream.id == virtual_model.upstream_model_id)
                else {
                    return false;
                };
                upstream_model.enabled
                    && config.providers.iter().any(|provider| {
                        provider.id == upstream_model.provider_id && provider.enabled
                    })
            })
            .cloned()
            .map(|mut virtual_model| {
                let upstream_model = config
                    .upstream_models
                    .iter()
                    .find(|upstream| upstream.id == virtual_model.upstream_model_id);
                if let Some(upstream_model) = upstream_model {
                    let provider = config
                        .providers
                        .iter()
                        .find(|provider| provider.id == upstream_model.provider_id);
                    if let Some(provider) = provider {
                        virtual_model.display_name = configured_model_display_name(
                            &virtual_model.display_name,
                            virtual_model.default_reasoning_level,
                            &provider.name,
                            upstream_model.capabilities.reasoning.supports_reasoning(),
                        );
                    }
                }
                virtual_model
            })
            .collect::<Vec<_>>();
        AntigravityModelDescriptor::inject_into_model_list(
            &mut base_json,
            &catalog_virtual_models,
            &config.upstream_models,
        );
        base_json
    }
}

fn effective_provider_request_timeout_ms(configured_timeout_ms: u64) -> u64 {
    match configured_timeout_ms {
        0 => DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS,
        configured => configured,
    }
}

fn effective_provider_connect_timeout_ms(
    configured_timeout_ms: u64,
    request_timeout_ms: u64,
) -> u64 {
    match configured_timeout_ms {
        0 => DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS,
        configured => configured,
    }
    .min(request_timeout_ms)
}

fn upstream_body_too_large_error() -> ProxyError {
    ProxyError::new(
        ErrorCategory::UpstreamServerError,
        format!(
            "Upstream response body exceeds {} bytes",
            DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES
        ),
        502,
    )
}

fn configured_model_display_name(
    model_name: &str,
    reasoning_level: Option<ReasoningLevel>,
    provider_name: &str,
    supports_reasoning: bool,
) -> String {
    let legacy_suffix = format!(" · {provider_name}");
    let provider_suffix = format!("({provider_name})");
    let mut base_name = model_name
        .strip_suffix(&legacy_suffix)
        .unwrap_or(model_name);
    for known_reasoning in [
        "default", "off", "low", "medium", "high", "xhigh", "max", "auto",
    ] {
        let known_suffix = format!(" {known_reasoning}({provider_name})");
        if let Some(stripped) = base_name.strip_suffix(&known_suffix) {
            base_name = stripped;
            break;
        }
    }
    base_name = base_name
        .strip_suffix(&provider_suffix)
        .unwrap_or(base_name);
    if !supports_reasoning {
        return format!("{base_name}{provider_suffix}");
    }

    let reasoning = match reasoning_level {
        None => "default",
        Some(ReasoningLevel::Off) => "off",
        Some(ReasoningLevel::Low) => "low",
        Some(ReasoningLevel::Medium) => "medium",
        Some(ReasoningLevel::High) => "high",
        Some(ReasoningLevel::XHigh) => "xhigh",
        Some(ReasoningLevel::Max) => "max",
        Some(ReasoningLevel::Auto) => "auto",
    };
    format!("{base_name} {reasoning}({provider_name})")
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
