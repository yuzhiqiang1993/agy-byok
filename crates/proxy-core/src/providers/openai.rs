use super::traits::ProviderAdapter;
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralMessage, NeutralStreamEvent, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct OpenAIAdapter;

impl OpenAIAdapter {
    pub fn new() -> Self {
        Self
    }

    fn convert_message(msg: &NeutralMessage) -> Value {
        let role_str = match msg.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };

        if msg.blocks.len() == 1 {
            if let NeutralContentBlock::Text(ref text) = msg.blocks[0] {
                return json!({
                    "role": role_str,
                    "content": text
                });
            }
        }

        let mut contents = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_id = None;

        for block in &msg.blocks {
            match block {
                NeutralContentBlock::Text(text) => {
                    contents.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
                NeutralContentBlock::Image {
                    mime_type,
                    data_base64,
                } => {
                    contents.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", mime_type, data_base64)
                        }
                    }));
                }
                NeutralContentBlock::ToolCall {
                    id,
                    name,
                    arguments_json,
                } => {
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments_json
                        }
                    }));
                }
                NeutralContentBlock::ToolResult {
                    tool_call_id: id,
                    content,
                } => {
                    tool_call_id = Some(id.clone());
                    contents.push(json!({
                        "type": "text",
                        "text": content
                    }));
                }
                NeutralContentBlock::Thinking { text, .. } => {
                    contents.push(json!({
                        "type": "text",
                        "text": format!("<thinking>\n{}\n</thinking>", text)
                    }));
                }
            }
        }

        let mut obj = json!({
            "role": role_str,
        });

        if msg.role == MessageRole::Tool {
            if let Some(id) = tool_call_id {
                obj["tool_call_id"] = json!(id);
            }
            if !contents.is_empty() {
                let first_text = contents[0]["text"].as_str().unwrap_or_default();
                obj["content"] = json!(first_text);
            }
        } else {
            if !contents.is_empty() {
                obj["content"] = Value::Array(contents);
            }
            if !tool_calls.is_empty() {
                obj["tool_calls"] = Value::Array(tool_calls);
            }
        }

        obj
    }
}

#[async_trait]
impl ProviderAdapter for OpenAIAdapter {
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<Value, ProxyError> {
        let mut payload = json!({
            "model": route.upstream_model.upstream_model_id,
            "stream": request.stream,
        });

        let mut messages_json = Vec::new();
        if let Some(ref sys) = request.system_instruction {
            messages_json.push(json!({
                "role": "system",
                "content": sys
            }));
        }

        for msg in &request.messages {
            messages_json.push(Self::convert_message(msg));
        }
        payload["messages"] = Value::Array(messages_json);

        if !request.tools.is_empty() {
            let tools_json: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.function.name,
                            "description": t.function.description,
                            "parameters": t.function.parameters_schema
                        }
                    })
                })
                .collect();
            payload["tools"] = Value::Array(tools_json);
        }

        let params = &route.final_parameters;
        if let Some(temp) = params.temperature {
            payload["temperature"] = json!(temp);
        }
        if let Some(max_t) = params.max_tokens {
            payload["max_tokens"] = json!(max_t);
        }
        if let Some(top_p) = params.top_p {
            payload["top_p"] = json!(top_p);
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
        if !api_key.is_empty() {
            headers.insert("Authorization".to_string(), format!("Bearer {}", api_key));
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
            return Err(
                ProxyError::new(cat, format!("OpenAI upstream status {}", status), status)
                    .with_upstream_body(body),
            );
        }

        let val: Value = serde_json::from_str(body).map_err(|e| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to parse OpenAI JSON response: {}", e),
                500,
            )
        })?;

        let id = val["id"].as_str().unwrap_or("gen-id").to_string();
        let model = val["model"]
            .as_str()
            .unwrap_or(&upstream_model.upstream_model_id)
            .to_string();

        let mut blocks = Vec::new();
        let mut finish_reason = None;

        if let Some(choice) = val["choices"].get(0) {
            finish_reason = choice["finish_reason"].as_str().map(|s| s.to_string());
            let message = &choice["message"];

            // 提炼 reasoning_content / thinking
            if let Some(reasoning) = message["reasoning_content"].as_str() {
                if !reasoning.is_empty() {
                    blocks.push(NeutralContentBlock::Thinking {
                        text: reasoning.to_string(),
                        signature: None,
                    });
                }
            }

            if let Some(content) = message["content"].as_str() {
                if !content.is_empty() {
                    blocks.push(NeutralContentBlock::Text(content.to_string()));
                }
            }

            if let Some(tool_calls) = message["tool_calls"].as_array() {
                for tc in tool_calls {
                    let tc_id = tc["id"].as_str().unwrap_or_default().to_string();
                    let func_name = tc["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let args = tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string();
                    blocks.push(NeutralContentBlock::ToolCall {
                        id: tc_id,
                        name: func_name,
                        arguments_json: args,
                    });
                }
            }
        }

        let usage = val["usage"].as_object().map(|u| UsageInfo {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
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
            if data_str == "[DONE]" {
                events.push(NeutralStreamEvent::Finish {
                    reason: "stop".to_string(),
                });
                continue;
            }

            let val: Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(choice) = val["choices"].get(0) {
                if let Some(finish_reason) = choice["finish_reason"].as_str() {
                    events.push(NeutralStreamEvent::Finish {
                        reason: finish_reason.to_string(),
                    });
                }

                let delta = &choice["delta"];
                if let Some(reasoning) = delta["reasoning_content"].as_str() {
                    if !reasoning.is_empty() {
                        events.push(NeutralStreamEvent::ThinkingDelta(reasoning.to_string()));
                    }
                }

                if let Some(content) = delta["content"].as_str() {
                    if !content.is_empty() {
                        events.push(NeutralStreamEvent::TextDelta(content.to_string()));
                    }
                }

                if let Some(tool_calls) = delta["tool_calls"].as_array() {
                    for tc in tool_calls {
                        let id = tc["id"].as_str().map(|s| s.to_string());
                        let name = tc["function"]["name"].as_str().map(|s| s.to_string());
                        let args_delta = tc["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        events.push(NeutralStreamEvent::ToolCallDelta {
                            id,
                            name,
                            arguments_delta: args_delta,
                        });
                    }
                }
            }

            if let Some(usage_obj) = val["usage"].as_object() {
                let usage = UsageInfo {
                    prompt_tokens: usage_obj
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    completion_tokens: usage_obj
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    total_tokens: usage_obj
                        .get("total_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                };
                events.push(NeutralStreamEvent::UsageUpdate(usage));
            }
        }

        Ok(events)
    }
}
