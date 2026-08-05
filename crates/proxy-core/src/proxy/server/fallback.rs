use super::activity_recorder::ActivityOutcome;
use super::execution::{CallbackFrameSink, EncodedFrameSink, StringFrameSink};
use super::ProxyServer;
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage,
    ParameterOverrides, ProxyError, ReasoningLevel, ReasoningMapping,
};
use crate::routing::RouteTable;
use std::time::Instant;

const CONNECTION_TEST_TIMEOUT_MS: u64 = 15_000;

impl ProxyServer {
    /// 发送最小非流式请求，验证指定模型的路由、鉴权和响应解析。
    pub async fn test_model_connection(&self, virtual_model_id: &str) -> Result<(), ProxyError> {
        self.test_model_connection_inner(virtual_model_id, None, true)
            .await
    }

    /// 发送保留指定推理等级的非流式请求，用于验证推理映射。
    pub async fn test_model_connection_with_reasoning(
        &self,
        virtual_model_id: &str,
        reasoning_level: ReasoningLevel,
    ) -> Result<(), ProxyError> {
        self.test_model_connection_inner(virtual_model_id, Some(reasoning_level), false)
            .await
    }

    async fn test_model_connection_inner(
        &self,
        virtual_model_id: &str,
        reasoning_level: Option<ReasoningLevel>,
        clear_default_reasoning: bool,
    ) -> Result<(), ProxyError> {
        let config = self.config_store.get_config();
        let request = NeutralChatRequest {
            virtual_model_id: virtual_model_id.to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Reply with OK.".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            reasoning_level,
            stream: false,
            generation_parameters: ParameterOverrides {
                max_tokens: if clear_default_reasoning {
                    Some(8)
                } else {
                    None
                },
                ..ParameterOverrides::default()
            },
            extra_body: Default::default(),
        };
        let mut route = RouteTable::resolve(&config, &request)?;
        if clear_default_reasoning {
            route.final_reasoning_level = None;
        } else if let Some(level) = route.final_reasoning_level {
            if let Some(ReasoningMapping::BudgetTokens(budget_tokens)) = route
                .upstream_model
                .capabilities
                .reasoning
                .mapping_for(level)
            {
                route.final_parameters.max_tokens = Some(budget_tokens.saturating_add(1));
            }
        }
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
}
