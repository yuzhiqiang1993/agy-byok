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
pub struct GeminiAdapter;

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }

    fn convert_message(msg: &NeutralMessage) -> Value {
        let role_str = match msg.role {
            MessageRole::User | MessageRole::System => "user",
            MessageRole::Assistant => "model",
            MessageRole::Tool => "function",
        };

        let mut parts = Vec::new();
        for b in &msg.blocks {
            match b {
                NeutralContentBlock::Text(text) => {
                    parts.push(json!({ "text": text }));
                }
                NeutralContentBlock::Image {
                    mime_type,
                    data_base64,
                } => {
                    parts.push(json!({
                        "inlineData": {
                            "mimeType": mime_type,
                            "data": data_base64
                        }
                    }));
                }
                NeutralContentBlock::ToolCall {
                    name,
                    arguments_json,
                    ..
                } => {
                    let args: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
                    parts.push(json!({
                        "functionCall": {
                            "name": name,
                            "args": args
                        }
                    }));
                }
                NeutralContentBlock::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    let response_val: Value = serde_json::from_str(content)
                        .unwrap_or_else(|_| json!({ "result": content }));
                    parts.push(json!({
                        "functionResponse": {
                            "name": tool_call_id,
                            "response": response_val
                        }
                    }));
                }
                NeutralContentBlock::Thinking { text, .. } => {
                    parts.push(json!({
                        "thought": true,
                        "text": text
                    }));
                }
            }
        }

        json!({
            "role": role_str,
            "parts": parts
        })
    }
}

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<Value, ProxyError> {
        let mut contents = Vec::new();
        for msg in &request.messages {
            contents.push(Self::convert_message(msg));
        }

        let mut payload = json!({
            "contents": contents
        });

        if let Some(ref sys) = request.system_instruction {
            payload["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        if !request.tools.is_empty() {
            let func_decls: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": t.function.parameters_schema
                    })
                })
                .collect();
            payload["tools"] = json!([{
                "functionDeclarations": func_decls
            }]);
        }

        let params = &route.final_parameters;
        let mut gen_config = json!({});
        if let Some(temp) = params.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(max_t) = params.max_tokens {
            gen_config["maxOutputTokens"] = json!(max_t);
        }
        if let Some(top_p) = params.top_p {
            gen_config["topP"] = json!(top_p);
        }
        if let Some(top_k) = params.top_k {
            gen_config["topK"] = json!(top_k);
        }

        if !gen_config.as_object().unwrap().is_empty() {
            payload["generationConfig"] = gen_config;
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
            headers.insert("x-goog-api-key".to_string(), api_key.to_string());
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
                ProxyError::new(cat, format!("Gemini upstream status {}", status), status)
                    .with_upstream_body(body),
            );
        }

        let val: Value = serde_json::from_str(body).map_err(|e| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to parse Gemini response: {}", e),
                500,
            )
        })?;

        let id = "gemini-resp".to_string();
        let model = upstream_model.upstream_model_id.clone();
        let mut blocks = Vec::new();
        let mut finish_reason = None;

        if let Some(candidate) = val["candidates"].get(0) {
            finish_reason = candidate["finishReason"].as_str().map(|s| s.to_string());
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    if part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        if let Some(t) = part["text"].as_str() {
                            blocks.push(NeutralContentBlock::Thinking {
                                text: t.to_string(),
                                signature: None,
                            });
                        }
                    } else if let Some(t) = part["text"].as_str() {
                        blocks.push(NeutralContentBlock::Text(t.to_string()));
                    } else if let Some(fc) = part.get("functionCall") {
                        let name = fc["name"].as_str().unwrap_or_default().to_string();
                        let args = fc["args"].to_string();
                        blocks.push(NeutralContentBlock::ToolCall {
                            id: format!("call_{}", name),
                            name,
                            arguments_json: args,
                        });
                    }
                }
            }
        }

        let usage = val["usageMetadata"].as_object().map(|u| UsageInfo {
            prompt_tokens: u
                .get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: u
                .get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u
                .get("totalTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
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

            if let Some(candidate) = val["candidates"].get(0) {
                if let Some(reason) = candidate["finishReason"].as_str() {
                    events.push(NeutralStreamEvent::Finish {
                        reason: reason.to_string(),
                    });
                }

                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for part in parts {
                        if part
                            .get("thought")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            if let Some(t) = part["text"].as_str() {
                                events.push(NeutralStreamEvent::ThinkingDelta(t.to_string()));
                            }
                        } else if let Some(t) = part["text"].as_str() {
                            events.push(NeutralStreamEvent::TextDelta(t.to_string()));
                        } else if let Some(fc) = part.get("functionCall") {
                            let name = fc["name"].as_str().map(|s| s.to_string());
                            let args = fc["args"].to_string();
                            events.push(NeutralStreamEvent::ToolCallDelta {
                                id: None,
                                name,
                                arguments_delta: args,
                            });
                        }
                    }
                }
            }

            if let Some(u) = val["usageMetadata"].as_object() {
                let usage = UsageInfo {
                    prompt_tokens: u
                        .get("promptTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    completion_tokens: u
                        .get("candidatesTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    total_tokens: u
                        .get("totalTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                };
                events.push(NeutralStreamEvent::UsageUpdate(usage));
            }
        }

        Ok(events)
    }
}
