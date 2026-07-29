use super::traits::ProviderAdapter;
use crate::domain::model::ReasoningMapping;
use crate::domain::response::{FinishReason, NeutralChoice};
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

    fn normalize_finish_reason(raw_finish_reason: &str) -> FinishReason {
        match raw_finish_reason {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::MaxTokens,
            "tool_calls" | "function_call" => FinishReason::ToolCall,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        }
    }

    fn parse_index(value: &Value, fallback: usize) -> u32 {
        value["index"]
            .as_u64()
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(fallback as u32)
    }

    fn parse_usage(value: &Value) -> Option<UsageInfo> {
        value.as_object().map(|usage| UsageInfo {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage
                .get("total_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
        })
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
        let mut reasoning_contents = Vec::new();
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
                    reasoning_contents.push(text.as_str());
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
            if !reasoning_contents.is_empty() {
                obj["reasoning_content"] = json!(reasoning_contents.join("\n"));
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

        if let Some(level) = route.final_reasoning_level {
            let mapping = route
                .upstream_model
                .capabilities
                .reasoning
                .mapping_for(level)
                .ok_or_else(|| {
                    ProxyError::new(
                        ErrorCategory::UnsupportedFeature,
                        format!(
                            "No reasoning mapping configured for OpenAI level {:?}",
                            level
                        ),
                        400,
                    )
                })?;
            match mapping {
                ReasoningMapping::Effort(effort) => {
                    payload["reasoning_effort"] = json!(effort);
                }
                ReasoningMapping::Disabled => {
                    payload["reasoning_effort"] = json!("none");
                }
                _ => {
                    return Err(ProxyError::new(
                        ErrorCategory::UnsupportedFeature,
                        format!("OpenAI does not support reasoning mapping: {:?}", mapping),
                        400,
                    ));
                }
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

        let mut choices = Vec::new();
        if let Some(upstream_choices) = val["choices"].as_array() {
            for (choice_position, choice) in upstream_choices.iter().enumerate() {
                let index = Self::parse_index(choice, choice_position);
                let message = &choice["message"];
                let mut blocks = Vec::new();

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
                    for tool_call in tool_calls {
                        let id = tool_call["id"].as_str().unwrap_or_default().to_string();
                        let name = tool_call["function"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let arguments_json = tool_call["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string();
                        blocks.push(NeutralContentBlock::ToolCall {
                            id,
                            name,
                            arguments_json,
                        });
                    }
                }

                let raw_finish_reason = choice["finish_reason"].as_str().map(str::to_string);
                let finish_reason = raw_finish_reason
                    .as_deref()
                    .map(Self::normalize_finish_reason);
                choices.push(NeutralChoice {
                    index,
                    blocks,
                    finish_reason,
                    raw_finish_reason,
                });
            }
        }

        Ok(NeutralChatResponse {
            id,
            model,
            choices,
            usage: Self::parse_usage(&val["usage"]),
        })
    }

    fn parse_stream_chunk(&self, chunk: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        let mut delta_events = Vec::new();
        let mut usage_events = Vec::new();
        let mut finish_events = Vec::new();

        for line in chunk.lines() {
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let data_str = line["data:".len()..].trim();
            if data_str == "[DONE]" {
                continue;
            }

            let val: Value = match serde_json::from_str(data_str) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if let Some(choices) = val["choices"].as_array() {
                for (choice_position, choice) in choices.iter().enumerate() {
                    let choice_index = Self::parse_index(choice, choice_position);
                    let delta = &choice["delta"];

                    if let Some(content) = delta["content"].as_str() {
                        if !content.is_empty() {
                            delta_events.push(NeutralStreamEvent::TextDelta {
                                choice_index,
                                text: content.to_string(),
                            });
                        }
                    }

                    if let Some(reasoning) = delta["reasoning_content"].as_str() {
                        if !reasoning.is_empty() {
                            delta_events.push(NeutralStreamEvent::ThinkingDelta {
                                choice_index,
                                text: reasoning.to_string(),
                            });
                        }
                    }

                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
                        for (tool_call_position, tool_call) in tool_calls.iter().enumerate() {
                            let tool_call_index = Self::parse_index(tool_call, tool_call_position);
                            let id = tool_call["id"].as_str().map(str::to_string);
                            let name = tool_call["function"]["name"].as_str().map(str::to_string);
                            let arguments_delta = tool_call["function"]["arguments"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();

                            delta_events.push(NeutralStreamEvent::ToolCallDelta {
                                choice_index,
                                tool_call_index,
                                id,
                                name,
                                arguments_delta,
                            });
                        }
                    }

                    if let Some(raw_finish_reason) = choice["finish_reason"].as_str() {
                        finish_events.push(NeutralStreamEvent::Finish {
                            choice_index,
                            reason: Self::normalize_finish_reason(raw_finish_reason),
                            raw_finish_reason: Some(raw_finish_reason.to_string()),
                        });
                    }
                }
            }

            if let Some(usage) = Self::parse_usage(&val["usage"]) {
                usage_events.push(NeutralStreamEvent::UsageUpdate(usage));
            }
        }

        delta_events.extend(usage_events);
        delta_events.extend(finish_events);
        Ok(delta_events)
    }
}
