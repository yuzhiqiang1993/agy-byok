use super::DEFAULT_MAX_TOKENS;
use crate::domain::model::ReasoningMapping;
use crate::domain::{
    input_modality_for_mime_type, ErrorCategory, MessageRole, ModelModality, NeutralChatRequest,
    NeutralContentBlock, Provider, ProxyError, TokenLimitSource,
};
use crate::routing::ResolvedRoute;
use serde_json::{json, Value};
use std::collections::HashMap;

// 只有目录或用户明确配置的上限才能约束自动生成的 Anthropic 输出预算。
fn trusted_output_token_limit(route: &ResolvedRoute) -> Option<u32> {
    matches!(
        route.upstream_model.token_limits.output_token_limit_source,
        TokenLimitSource::Catalog | TokenLimitSource::Configured
    )
    .then_some(route.upstream_model.token_limits.output_token_limit)
    .flatten()
}

fn convert_blocks(blocks: &[NeutralContentBlock]) -> Result<Vec<Value>, ProxyError> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            NeutralContentBlock::Text(text) => {
                out.push(json!({
                    "type": "text",
                    "text": text
                }));
            }
            NeutralContentBlock::InlineData {
                mime_type,
                data_base64,
            } => {
                if input_modality_for_mime_type(mime_type) == ModelModality::Image {
                    out.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": mime_type,
                            "data": data_base64
                        }
                    }));
                } else {
                    out.push(json!({
                        "type": "document",
                        "source": {
                            "type": "base64",
                            "media_type": mime_type,
                            "data": data_base64
                        }
                    }));
                }
            }
            NeutralContentBlock::ToolCall {
                id,
                name,
                arguments_json,
            } => {
                let input_val: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
                out.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input_val
                }));
            }
            NeutralContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                out.push(json!({
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content
                }));
            }
            NeutralContentBlock::Thinking { text, signature } => {
                let mut obj = json!({
                    "type": "thinking",
                    "thinking": text
                });
                if let Some(sig) = signature {
                    obj["signature"] = json!(sig);
                }
                out.push(obj);
            }
        }
    }
    Ok(out)
}

pub(super) fn build_request_payload(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<Value, ProxyError> {
    let max_tokens = route
        .final_parameters
        .max_tokens
        .unwrap_or(DEFAULT_MAX_TOKENS);

    let mut payload = json!({
        "model": route.upstream_model.upstream_model_id,
        "max_tokens": max_tokens,
        "stream": request.stream,
    });

    if let Some(ref sys) = request.system_instruction {
        payload["system"] = json!(sys);
    }

    let mut messages_json = Vec::new();
    for msg in &request.messages {
        let role_str = match msg.role {
            MessageRole::User | MessageRole::System => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "user",
        };

        let content_blocks = convert_blocks(&msg.blocks)?;
        messages_json.push(json!({
            "role": role_str,
            "content": content_blocks
        }));
    }
    payload["messages"] = Value::Array(messages_json);

    if !request.tools.is_empty() {
        let tools_json: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "input_schema": tool.function.parameters_schema
                })
            })
            .collect();
        payload["tools"] = Value::Array(tools_json);
    }

    let params = &route.final_parameters;
    if let Some(temp) = params.temperature {
        payload["temperature"] = json!(temp);
    }
    if let Some(top_p) = params.top_p {
        payload["top_p"] = json!(top_p);
    }
    if let Some(top_k) = params.top_k {
        payload["top_k"] = json!(top_k);
    }

    if let Some(ref extra) = params.extra_body {
        for (k, v) in extra {
            payload[k] = v.clone();
        }
    }

    if let Some(level) = route.final_reasoning_level {
        match route
            .upstream_model
            .capabilities
            .reasoning
            .mapping_for(level)
        {
            Some(ReasoningMapping::BudgetTokens(budget_tokens)) => {
                match route.final_parameters.max_tokens {
                    Some(max_tokens) if max_tokens <= *budget_tokens => {
                        return Err(ProxyError::new(
                            ErrorCategory::InvalidRequest,
                            format!(
                                "Anthropic max_tokens ({max_tokens}) must be greater than thinking budget ({budget_tokens})"
                            ),
                            400,
                        ));
                    }
                    None => {
                        let generated_max_tokens = budget_tokens.saturating_add(DEFAULT_MAX_TOKENS);
                        let effective_max_tokens = match trusted_output_token_limit(route) {
                            Some(output_limit) if output_limit <= *budget_tokens => {
                                return Err(ProxyError::new(
                                    ErrorCategory::InvalidRequest,
                                    format!(
                                        "Anthropic output token limit ({output_limit}) must be greater than thinking budget ({budget_tokens})"
                                    ),
                                    400,
                                ));
                            }
                            Some(output_limit) => generated_max_tokens.min(output_limit),
                            None => generated_max_tokens,
                        };
                        payload["max_tokens"] = Value::from(effective_max_tokens);
                    }
                    Some(_) => {}
                }
                payload["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": budget_tokens
                });
            }
            Some(ReasoningMapping::Adaptive) => {
                payload["thinking"] = json!({ "type": "adaptive" });
            }
            Some(ReasoningMapping::Effort(effort)) => {
                payload["thinking"] = json!({ "type": "adaptive" });
                match payload.get_mut("output_config") {
                    Some(Value::Object(output_config)) => {
                        output_config.insert("effort".to_string(), json!(effort));
                    }
                    _ => {
                        payload["output_config"] = json!({ "effort": effort });
                    }
                }
            }
            Some(ReasoningMapping::Disabled) => {
                payload["thinking"] = json!({ "type": "disabled" });
            }
            Some(mapping) => {
                return Err(ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!(
                        "Anthropic does not support reasoning mapping {:?} for level {:?}",
                        mapping, level
                    ),
                    400,
                ));
            }
            None => {
                return Err(ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!(
                        "No reasoning mapping configured for Anthropic reasoning level {:?}",
                        level
                    ),
                    400,
                ));
            }
        }
    }

    Ok(payload)
}

pub(super) fn build_headers(provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
    if !provider.api_key.is_empty() {
        headers.insert("x-api-key".to_string(), provider.api_key.clone());
    }

    for (k, v) in &provider.headers {
        headers.insert(k.clone(), v.clone());
    }

    Ok(headers)
}
