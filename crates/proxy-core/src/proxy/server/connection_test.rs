use super::ProxyServer;
use crate::domain::{
    ConnectionTestContext, ErrorCategory, MessageRole, ModelModality, ModelRole,
    NeutralChatRequest, NeutralContentBlock, NeutralMessage, ParameterOverrides, ProxyError,
    ReasoningLevel, ReasoningMapping,
};
use crate::providers::get_adapter;
use crate::routing::RouteTable;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use std::collections::BTreeSet;

impl ProxyServer {
    /// 发送最小非流式请求，验证指定模型的路由、鉴权和响应解析。
    pub async fn test_model_connection(
        &self,
        virtual_model_id: &str,
    ) -> Result<ConnectionTestContext, ProxyError> {
        self.test_model_connection_inner(virtual_model_id, None, true)
            .await
    }

    /// 发送保留指定推理等级的非流式请求，用于验证推理映射。
    pub async fn test_model_connection_with_reasoning(
        &self,
        virtual_model_id: &str,
        reasoning_level: ReasoningLevel,
    ) -> Result<ConnectionTestContext, ProxyError> {
        self.test_model_connection_inner(virtual_model_id, Some(reasoning_level), false)
            .await
    }

    async fn test_model_connection_inner(
        &self,
        virtual_model_id: &str,
        reasoning_level: Option<ReasoningLevel>,
        clear_default_reasoning: bool,
    ) -> Result<ConnectionTestContext, ProxyError> {
        let config = self.config_store.get_config();
        let target_is_image_only = config
            .virtual_models
            .iter()
            .find(|vm| vm.matches_id(virtual_model_id))
            .and_then(|vm| {
                config
                    .upstream_models
                    .iter()
                    .find(|um| um.id == vm.upstream_model_id)
            })
            .map(|um| {
                um.capabilities.roles.contains(&ModelRole::ImageGeneration)
                    && !um.capabilities.roles.contains(&ModelRole::Agent)
            })
            .unwrap_or(false);

        let request = if target_is_image_only {
            NeutralChatRequest {
                virtual_model_id: virtual_model_id.to_string(),
                messages: vec![NeutralMessage {
                    role: MessageRole::User,
                    blocks: vec![NeutralContentBlock::Text("a small red dot".to_string())],
                }],
                system_instruction: None,
                tools: vec![],
                output_modalities: BTreeSet::from([ModelModality::Image]),
                image_generation_config: None,
                reasoning_level: None,
                stream: false,
                generation_parameters: ParameterOverrides::default(),
                extra_body: Default::default(),
            }
        } else {
            NeutralChatRequest {
                virtual_model_id: virtual_model_id.to_string(),
                messages: vec![NeutralMessage {
                    role: MessageRole::User,
                    blocks: vec![NeutralContentBlock::Text("Reply with OK.".to_string())],
                }],
                system_instruction: None,
                tools: vec![],
                output_modalities: Default::default(),
                image_generation_config: None,
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
            }
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
        let is_image_model = route
            .upstream_model
            .capabilities
            .roles
            .contains(&ModelRole::ImageGeneration)
            || route
                .upstream_model
                .capabilities
                .supports_output(ModelModality::Image);

        let test_timeout = if is_image_model { 60_000 } else { 30_000 };

        route.provider.request_timeout_ms = route.provider.request_timeout_ms.max(test_timeout);

        let adapter = get_adapter(&route.provider.protocol);
        let request_body_str = match adapter.build_request_payload(&route, &request) {
            Ok(payload) => serde_json::to_string_pretty(&payload).ok(),
            Err(e) => {
                return Ok(ConnectionTestContext {
                    success: false,
                    request_body: None,
                    response_body: None,
                    status_code: Some(e.status_code),
                    error_category: Some(e.category),
                    error_message: Some(e.message),
                })
            }
        };

        match self.send_upstream(&route, &request).await {
            Ok((_, response)) => {
                let status = response.status().as_u16();
                let buffered = match read_limited_response_body(
                    response,
                    DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES,
                )
                .await
                {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(ConnectionTestContext {
                            success: false,
                            request_body: request_body_str,
                            response_body: None,
                            status_code: Some(status),
                            error_category: Some(ErrorCategory::Internal),
                            error_message: Some(format!("Failed to read body: {e}")),
                        })
                    }
                };
                let truncated = buffered.is_truncated();
                let mut body = buffered.into_text();
                if truncated {
                    body.push_str("\n[upstream response body exceeded the buffered limit]");
                }

                if status < 400 {
                    // Check adapter parsing capability if it is successful HTTP request
                    match adapter.parse_response(status, &body, &route.upstream_model) {
                        Ok(_) => Ok(ConnectionTestContext {
                            success: true,
                            request_body: request_body_str,
                            response_body: Some(body),
                            status_code: Some(status),
                            error_category: None,
                            error_message: None,
                        }),
                        Err(e) => Ok(ConnectionTestContext {
                            success: false,
                            request_body: request_body_str,
                            response_body: Some(body),
                            status_code: Some(e.status_code),
                            error_category: Some(e.category),
                            error_message: Some(e.message),
                        }),
                    }
                } else {
                    Ok(ConnectionTestContext {
                        success: false,
                        request_body: request_body_str,
                        response_body: Some(body),
                        status_code: Some(status),
                        error_category: Some(ErrorCategory::UpstreamServerError),
                        error_message: Some(format!("Upstream returned HTTP {}", status)),
                    })
                }
            }
            Err(e) => Ok(ConnectionTestContext {
                success: false,
                request_body: request_body_str,
                response_body: e.upstream_body,
                status_code: Some(e.status_code),
                error_category: Some(e.category),
                error_message: Some(e.message),
            }),
        }
    }
}
