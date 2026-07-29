use super::traits::ProviderAdapter;
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralStreamEvent, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct AnthropicAdapter;

impl AnthropicAdapter {
    pub fn new() -> Self {
        Self
    }

    fn convert_blocks(blocks: &[NeutralContentBlock]) -> Vec<Value> {
        let mut out = Vec::new();
        for b in blocks {
            match b {
                NeutralContentBlock::Text(text) => {
                    out.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
                NeutralContentBlock::Image {
                    mime_type,
                    data_base64,
                } => {
                    out.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": mime_type,
                            "data": data_base64
                        }
                    }));
                }
                NeutralContentBlock::ToolCall {
                    id,
                    name,
                    arguments_json,
                } => {
                    let input_val: Value =
                        serde_json::from_str(arguments_json).unwrap_or(json!({}));
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
        out
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<Value, ProxyError> {
        let max_tokens = route.final_parameters.max_tokens.unwrap_or(4096);

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

            let content_blocks = Self::convert_blocks(&msg.blocks);
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
                .map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters_schema
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

        Ok(payload)
    }

    fn build_headers(
        &self,
        provider: &Provider,
        api_key: &str,
    ) -> Result<HashMap<String, String>, ProxyError> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        if !api_key.is_empty() {
            headers.insert("x-api-key".to_string(), api_key.to_string());
        }

        for (k, v) in &provider.headers {
            headers.insert(k.clone(), v.clone());
        }

        Ok(headers)
    }

    fn parse_response(
        &self,
        status: u16,
        body: &str,
        upstream_model: &UpstreamModel,
    ) -> Result<NeutralChatResponse, ProxyError> {
        if status >= 400 {
            let cat = match status {
                401 | 403 => ErrorCategory::Authentication,
                404 => ErrorCategory::ModelNotFound,
                429 => ErrorCategory::RateLimit,
                500..=599 => ErrorCategory::UpstreamServerError,
                _ => ErrorCategory::InvalidRequest,
            };
            return Err(ProxyError::new(
                cat,
                format!("Anthropic upstream status {}", status),
                status,
            )
            .with_upstream_body(body));
        }

        let val: Value = serde_json::from_str(body).map_err(|e| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to parse Anthropic response: {}", e),
                500,
            )
        })?;

        let id = val["id"].as_str().unwrap_or("msg-id").to_string();
        let model = val["model"]
            .as_str()
            .unwrap_or(&upstream_model.upstream_model_id)
            .to_string();
        let finish_reason = val["stop_reason"].as_str().map(|s| s.to_string());

        let mut blocks = Vec::new();
        if let Some(contents) = val["content"].as_array() {
            for item in contents {
                let item_type = item["type"].as_str().unwrap_or_default();
                match item_type {
                    "text" => {
                        if let Some(t) = item["text"].as_str() {
                            blocks.push(NeutralContentBlock::Text(t.to_string()));
                        }
                    }
                    "thinking" => {
                        let text = item["thinking"].as_str().unwrap_or_default();
                        let sig = item["signature"].as_str().map(|s| s.to_string());
                        blocks.push(NeutralContentBlock::Thinking {
                            text: text.to_string(),
                            signature: sig,
                        });
                    }
                    "tool_use" => {
                        let id = item["id"].as_str().unwrap_or_default().to_string();
                        let name = item["name"].as_str().unwrap_or_default().to_string();
                        let input_str = serde_json::to_string(&item["input"]).unwrap_or_default();
                        blocks.push(NeutralContentBlock::ToolCall {
                            id,
                            name,
                            arguments_json: input_str,
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = val["usage"].as_object().map(|u| UsageInfo {
            prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: (u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0))
                as u32,
        });

        Ok(NeutralChatResponse {
            id,
            model,
            choices_blocks: blocks,
            usage,
            finish_reason,
        })
    }

    fn parse_stream_chunk(&self, chunk: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        let mut events = Vec::new();
        for line in chunk.lines() {
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let data_str = line["data:".len()..].trim();
            let val: Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let event_type = val["type"].as_str().unwrap_or_default();
            match event_type {
                "content_block_delta" => {
                    let delta = &val["delta"];
                    let delta_type = delta["type"].as_str().unwrap_or_default();
                    if delta_type == "text_delta" {
                        if let Some(t) = delta["text"].as_str() {
                            events.push(NeutralStreamEvent::TextDelta(t.to_string()));
                        }
                    } else if delta_type == "thinking_delta" {
                        if let Some(t) = delta["thinking"].as_str() {
                            events.push(NeutralStreamEvent::ThinkingDelta(t.to_string()));
                        }
                    } else if delta_type == "input_json_delta" {
                        if let Some(partial) = delta["partial_json"].as_str() {
                            events.push(NeutralStreamEvent::ToolCallDelta {
                                id: None,
                                name: None,
                                arguments_delta: partial.to_string(),
                            });
                        }
                    }
                }
                "message_delta" => {
                    if let Some(reason) = val["delta"]["stop_reason"].as_str() {
                        events.push(NeutralStreamEvent::Finish {
                            reason: reason.to_string(),
                        });
                    }
                    if let Some(usage_obj) = val["usage"].as_object() {
                        let comp_tokens = usage_obj
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        events.push(NeutralStreamEvent::UsageUpdate(UsageInfo {
                            prompt_tokens: 0,
                            completion_tokens: comp_tokens,
                            total_tokens: comp_tokens,
                        }));
                    }
                }
                _ => {}
            }
        }

        Ok(events)
    }
}
