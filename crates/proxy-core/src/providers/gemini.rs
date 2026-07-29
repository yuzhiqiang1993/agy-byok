use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::model::ReasoningMapping;
use crate::domain::response::{FinishReason, NeutralChoice};
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralMessage, NeutralStreamEvent, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

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
            MessageRole::Tool => "user",
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

    fn write_reasoning(payload: &mut Value, route: &ResolvedRoute) -> Result<(), ProxyError> {
        let Some(level) = route.final_reasoning_level else {
            return Ok(());
        };

        let mapping = route
            .upstream_model
            .capabilities
            .reasoning
            .mapping_for(level)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!(
                        "No reasoning mapping configured for Gemini level {:?}",
                        level
                    ),
                    400,
                )
            })?;

        let (field, value) = match mapping {
            ReasoningMapping::Disabled => ("thinkingBudget", json!(0)),
            ReasoningMapping::BudgetTokens(tokens) => ("thinkingBudget", json!(tokens)),
            ReasoningMapping::NativeLevel(level) => ("thinkingLevel", json!(level)),
            ReasoningMapping::Effort(_) | ReasoningMapping::Adaptive => {
                return Err(ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!("Gemini does not support reasoning mapping {:?}", mapping),
                    400,
                ));
            }
        };

        let generation_config = payload
            .as_object_mut()
            .expect("Gemini payload must be an object")
            .entry("generationConfig")
            .or_insert_with(|| json!({}));
        if !generation_config.is_object() {
            *generation_config = json!({});
        }

        let thinking_config = generation_config
            .as_object_mut()
            .expect("generationConfig must be an object")
            .entry("thinkingConfig")
            .or_insert_with(|| json!({}));
        if !thinking_config.is_object() {
            *thinking_config = json!({});
        }
        let thinking_config = thinking_config
            .as_object_mut()
            .expect("thinkingConfig must be an object");
        if field == "thinkingBudget" {
            thinking_config.remove("thinkingLevel");
        } else {
            thinking_config.remove("thinkingBudget");
        }
        thinking_config.insert(field.to_string(), value);

        Ok(())
    }

    fn normalize_finish_reason(reason: &str) -> FinishReason {
        match reason {
            "STOP" => FinishReason::Stop,
            "MAX_TOKENS" => FinishReason::MaxTokens,
            "TOOL_CALL" => FinishReason::ToolCall,
            "SAFETY"
            | "BLOCKLIST"
            | "PROHIBITED_CONTENT"
            | "SPII"
            | "IMAGE_SAFETY"
            | "IMAGE_PROHIBITED_CONTENT" => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        }
    }
}

struct GeminiStreamDecoder {
    model: String,
    response_started: bool,
    response_ended: bool,
    emitted_tool_calls: HashSet<(u32, u32)>,
    finished_choices: HashSet<u32>,
}

impl GeminiStreamDecoder {
    fn new(model: String) -> Self {
        Self {
            model,
            response_started: false,
            response_ended: false,
            emitted_tool_calls: HashSet::new(),
            finished_choices: HashSet::new(),
        }
    }

    fn response_end(&mut self) -> Vec<NeutralStreamEvent> {
        if self.response_ended {
            Vec::new()
        } else {
            self.response_ended = true;
            vec![NeutralStreamEvent::ResponseEnd]
        }
    }
}

impl ProviderStreamDecoder for GeminiStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        if data.trim() == "[DONE]" {
            return Ok(self.response_end());
        }

        let value: Value = serde_json::from_str(data).map_err(|error| {
            ProxyError::new(
                ErrorCategory::StreamInterrupted,
                format!("Failed to parse Gemini stream data: {error}"),
                502,
            )
        })?;

        let mut events = Vec::new();
        if !self.response_started {
            self.response_started = true;
            events.push(NeutralStreamEvent::ResponseStart {
                response_id: value
                    .get("responseId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                model: self.model.clone(),
            });
        }

        let mut finish_events = Vec::new();
        if let Some(candidates) = value.get("candidates").and_then(Value::as_array) {
            for (candidate_position, candidate) in candidates.iter().enumerate() {
                let choice_index = candidate
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or(candidate_position as u32);

                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
                {
                    for (part_position, part) in parts.iter().enumerate() {
                        let part_index = part_position as u32;
                        if part
                            .get("thought")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                events.push(NeutralStreamEvent::ThinkingDelta {
                                    choice_index,
                                    text: text.to_string(),
                                });
                            }
                        } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                            events.push(NeutralStreamEvent::TextDelta {
                                choice_index,
                                text: text.to_string(),
                            });
                        } else if let Some(function_call) = part.get("functionCall") {
                            if self.emitted_tool_calls.insert((choice_index, part_index)) {
                                let name = function_call
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                let arguments = function_call
                                    .get("args")
                                    .cloned()
                                    .unwrap_or(Value::Null)
                                    .to_string();
                                events.push(NeutralStreamEvent::ToolCallStart {
                                    choice_index,
                                    tool_call_index: part_index,
                                    id: format!("call_{choice_index}_{part_index}"),
                                    name,
                                });
                                events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                                    choice_index,
                                    tool_call_index: part_index,
                                    arguments_delta: arguments,
                                });
                                events.push(NeutralStreamEvent::ToolCallEnd {
                                    choice_index,
                                    tool_call_index: part_index,
                                });
                            }
                        }
                    }
                }

                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    if self.finished_choices.insert(choice_index) {
                        finish_events.push(NeutralStreamEvent::Finish {
                            choice_index,
                            reason: GeminiAdapter::normalize_finish_reason(reason),
                            raw_finish_reason: Some(reason.to_string()),
                        });
                    }
                }
            }
        }

        if let Some(usage) = value.get("usageMetadata").and_then(Value::as_object) {
            events.push(NeutralStreamEvent::UsageUpdate(UsageInfo {
                prompt_tokens: usage
                    .get("promptTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                completion_tokens: usage
                    .get("candidatesTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                total_tokens: usage
                    .get("totalTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
            }));
        }

        events.extend(finish_events);
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(self.response_end())
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

        Self::write_reasoning(&mut payload, route)?;

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
        let mut choices = Vec::new();

        if let Some(candidates) = val["candidates"].as_array() {
            for (candidate_position, candidate) in candidates.iter().enumerate() {
                let choice_index = candidate["index"]
                    .as_u64()
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or(candidate_position as u32);
                let raw_finish_reason = candidate["finishReason"].as_str().map(ToString::to_string);
                let finish_reason = raw_finish_reason
                    .as_deref()
                    .map(Self::normalize_finish_reason);
                let mut blocks = Vec::new();

                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for (part_position, part) in parts.iter().enumerate() {
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
                                id: format!("call_{}_{}", candidate_position, part_position),
                                name,
                                arguments_json: args,
                            });
                        }
                    }
                }

                choices.push(NeutralChoice {
                    index: choice_index,
                    blocks,
                    finish_reason,
                    raw_finish_reason,
                });
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
            choices,
            usage,
        })
    }

    fn create_stream_decoder(
        &self,
        upstream_model: &UpstreamModel,
    ) -> Box<dyn ProviderStreamDecoder> {
        Box::new(GeminiStreamDecoder::new(
            upstream_model.upstream_model_id.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_messages_are_encoded_as_user_turns() {
        let message = NeutralMessage {
            role: MessageRole::Tool,
            blocks: vec![NeutralContentBlock::ToolResult {
                tool_call_id: "lookup".to_string(),
                content: r#"{"result":"ok"}"#.to_string(),
            }],
        };

        let converted = GeminiAdapter::convert_message(&message);

        assert_eq!(converted["role"], "user");
    }

    #[test]
    fn decoder_emits_all_candidates_with_usage_before_finish() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());

        let events = decoder
            .decode_data(
                r#"{
                    "responseId":"response-1",
                    "candidates":[
                        {
                            "index":4,
                            "content":{"parts":[
                                {"text":"answer"},
                                {"thought":true,"text":"reason"},
                                {"functionCall":{"name":"lookup","args":{"query":"rust"}}}
                            ]},
                            "finishReason":"TOOL_CALL"
                        },
                        {
                            "content":{"parts":[{"text":"alternative"}]},
                            "finishReason":"MAX_TOKENS"
                        }
                    ],
                    "usageMetadata":{
                        "promptTokenCount":3,
                        "candidatesTokenCount":5,
                        "totalTokenCount":8
                    }
                }"#,
            )
            .unwrap();

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: Some("response-1".to_string()),
                    model: "gemini-upstream".to_string(),
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 4,
                    text: "answer".to_string(),
                },
                NeutralStreamEvent::ThinkingDelta {
                    choice_index: 4,
                    text: "reason".to_string(),
                },
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 4,
                    tool_call_index: 2,
                    id: "call_4_2".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 4,
                    tool_call_index: 2,
                    arguments_delta: r#"{"query":"rust"}"#.to_string(),
                },
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 4,
                    tool_call_index: 2,
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 1,
                    text: "alternative".to_string(),
                },
                NeutralStreamEvent::UsageUpdate(UsageInfo {
                    prompt_tokens: 3,
                    completion_tokens: 5,
                    total_tokens: 8,
                }),
                NeutralStreamEvent::Finish {
                    choice_index: 4,
                    reason: FinishReason::ToolCall,
                    raw_finish_reason: Some("TOOL_CALL".to_string()),
                },
                NeutralStreamEvent::Finish {
                    choice_index: 1,
                    reason: FinishReason::MaxTokens,
                    raw_finish_reason: Some("MAX_TOKENS".to_string()),
                },
            ]
        );
    }

    #[test]
    fn decoder_deduplicates_tool_parts_and_choice_finish() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        let data = r#"{
            "candidates":[{
                "index":2,
                "content":{"parts":[{
                    "functionCall":{"name":"lookup","args":{"query":"rust"}}
                }]},
                "finishReason":"STOP"
            }]
        }"#;

        let first_events = decoder.decode_data(data).unwrap();
        let repeated_events = decoder.decode_data(data).unwrap();

        assert_eq!(first_events.len(), 5);
        assert!(repeated_events.is_empty());
    }

    #[test]
    fn decoder_ends_once_for_done_or_eof() {
        let mut done_decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            done_decoder.decode_data("[DONE]").unwrap(),
            vec![NeutralStreamEvent::ResponseEnd]
        );
        assert!(done_decoder.decode_data("{}").unwrap().is_empty());
        assert!(done_decoder.finish().unwrap().is_empty());

        let mut eof_decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            eof_decoder.finish().unwrap(),
            vec![NeutralStreamEvent::ResponseEnd]
        );
        assert!(eof_decoder.finish().unwrap().is_empty());
    }

    #[test]
    fn decoder_starts_without_response_id_and_rejects_invalid_json() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            decoder.decode_data("{}").unwrap(),
            vec![NeutralStreamEvent::ResponseStart {
                response_id: None,
                model: "gemini-upstream".to_string(),
            }]
        );

        let error = decoder.decode_data("data: {}").unwrap_err();
        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
        assert_eq!(error.status_code, 502);
    }
}
