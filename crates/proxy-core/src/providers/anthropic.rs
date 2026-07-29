use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::model::ReasoningMapping;
use crate::domain::response::{FinishReason, NeutralChoice};
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralStreamEvent, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicContentBlockKind {
    Text,
    Thinking,
    ToolUse,
    Other,
}

struct AnthropicStreamDecoder {
    fallback_model: String,
    message_started: bool,
    finish_emitted: bool,
    response_ended: bool,
    usage: UsageInfo,
    open_blocks: BTreeMap<u32, AnthropicContentBlockKind>,
}

impl AnthropicStreamDecoder {
    fn new(upstream_model: &UpstreamModel) -> Self {
        Self {
            fallback_model: upstream_model.upstream_model_id.clone(),
            message_started: false,
            finish_emitted: false,
            response_ended: false,
            usage: UsageInfo::default(),
            open_blocks: BTreeMap::new(),
        }
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }

    fn ensure_active_message(&self, event_type: &str) -> Result<(), ProxyError> {
        if self.response_ended {
            return Err(Self::stream_error(format!(
                "Anthropic {event_type} received after response end"
            )));
        }
        if !self.message_started {
            return Err(Self::stream_error(format!(
                "Anthropic {event_type} received before message_start"
            )));
        }
        Ok(())
    }

    fn required_object<'a>(
        value: &'a Value,
        field: &str,
        event_type: &str,
    ) -> Result<&'a serde_json::Map<String, Value>, ProxyError> {
        value.get(field).and_then(Value::as_object).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} is missing object field {field}"
            ))
        })
    }

    fn required_index(value: &Value, event_type: &str) -> Result<u32, ProxyError> {
        let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} is missing integer field index"
            ))
        })?;
        u32::try_from(index).map_err(|_| {
            Self::stream_error(format!(
                "Anthropic {event_type} index exceeds the supported range"
            ))
        })
    }

    fn parse_usage(
        value: Option<&Value>,
        current: &UsageInfo,
        event_type: &str,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        let usage = value.as_object().ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} usage must be a JSON object"
            ))
        })?;

        let parse_token = |field: &str| -> Result<Option<u32>, ProxyError> {
            let Some(value) = usage.get(field) else {
                return Ok(None);
            };
            let token_count = value.as_u64().ok_or_else(|| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage.{field} must be a non-negative integer"
                ))
            })?;
            u32::try_from(token_count).map(Some).map_err(|_| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage.{field} exceeds the supported range"
                ))
            })
        };

        let prompt_tokens = parse_token("input_tokens")?.unwrap_or(current.prompt_tokens);
        let completion_tokens = parse_token("output_tokens")?.unwrap_or(current.completion_tokens);
        let total_tokens = prompt_tokens
            .checked_add(completion_tokens)
            .ok_or_else(|| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage token total exceeds the supported range"
                ))
            })?;

        Ok(Some(UsageInfo {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }))
    }

    fn require_block_kind(
        &self,
        index: u32,
        expected: AnthropicContentBlockKind,
        delta_type: &str,
    ) -> Result<(), ProxyError> {
        match self.open_blocks.get(&index) {
            Some(kind) if *kind == expected => Ok(()),
            Some(_) => Err(Self::stream_error(format!(
                "Anthropic {delta_type} does not match content block {index}"
            ))),
            None => Err(Self::stream_error(format!(
                "Anthropic {delta_type} references unopened content block {index}"
            ))),
        }
    }

    fn decode_message_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Err(Self::stream_error(
                "Anthropic message_start received after response end",
            ));
        }
        if self.message_started {
            return Err(Self::stream_error(
                "Anthropic stream contains multiple message_start events",
            ));
        }

        let message = Self::required_object(value, "message", "message_start")?;
        let response_id = match message.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.clone()),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_start message.id must be a string",
                ));
            }
        };
        let model = match message.get("model") {
            None | Some(Value::Null) => self.fallback_model.clone(),
            Some(Value::String(model)) => model.clone(),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_start message.model must be a string",
                ));
            }
        };
        let usage = Self::parse_usage(message.get("usage"), &self.usage, "message_start")?;

        if let Some(usage) = usage {
            self.usage = usage;
        }
        self.message_started = true;

        Ok(vec![NeutralStreamEvent::ResponseStart {
            response_id,
            model,
        }])
    }

    fn decode_content_block_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_start")?;
        let index = Self::required_index(value, "content_block_start")?;
        if self.open_blocks.contains_key(&index) {
            return Err(Self::stream_error(format!(
                "Anthropic content block {index} was started more than once"
            )));
        }

        let content_block = Self::required_object(value, "content_block", "content_block_start")?;
        let block_type = content_block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Self::stream_error(
                    "Anthropic content_block_start is missing string field content_block.type",
                )
            })?;

        match block_type {
            "tool_use" => {
                let id = content_block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic tool_use content block is missing string field id",
                        )
                    })?
                    .to_string();
                let name = content_block
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic tool_use content block is missing string field name",
                        )
                    })?
                    .to_string();
                let initial_arguments = match content_block.get("input") {
                    None | Some(Value::Null) => None,
                    Some(Value::Object(input)) if input.is_empty() => None,
                    Some(input @ Value::Object(_)) => {
                        Some(serde_json::to_string(input).map_err(|error| {
                            Self::stream_error(format!(
                                "Failed to encode Anthropic tool_use input: {error}"
                            ))
                        })?)
                    }
                    Some(_) => {
                        return Err(Self::stream_error(
                            "Anthropic tool_use content block input must be a JSON object",
                        ));
                    }
                };

                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::ToolUse);
                let mut events = vec![NeutralStreamEvent::ToolCallStart {
                    choice_index: 0,
                    tool_call_index: index,
                    id,
                    name,
                }];
                if let Some(arguments_delta) = initial_arguments {
                    events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                        choice_index: 0,
                        tool_call_index: index,
                        arguments_delta,
                    });
                }
                Ok(events)
            }
            "text" => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Text);
                Ok(Vec::new())
            }
            "thinking" => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Thinking);
                Ok(Vec::new())
            }
            _ => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Other);
                Ok(Vec::new())
            }
        }
    }

    fn decode_content_block_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_delta")?;
        let delta = Self::required_object(value, "delta", "content_block_delta")?;
        let delta_type = delta.get("type").and_then(Value::as_str).ok_or_else(|| {
            Self::stream_error("Anthropic content_block_delta is missing string field delta.type")
        })?;

        match delta_type {
            "text_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::Text, delta_type)?;
                let text = delta.get("text").and_then(Value::as_str).ok_or_else(|| {
                    Self::stream_error("Anthropic text_delta is missing string field text")
                })?;
                Ok(vec![NeutralStreamEvent::TextDelta {
                    choice_index: 0,
                    text: text.to_string(),
                }])
            }
            "thinking_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::Thinking, delta_type)?;
                let text = delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic thinking_delta is missing string field thinking",
                        )
                    })?;
                Ok(vec![NeutralStreamEvent::ThinkingDelta {
                    choice_index: 0,
                    text: text.to_string(),
                }])
            }
            "input_json_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::ToolUse, delta_type)?;
                let partial_json = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic input_json_delta is missing string field partial_json",
                        )
                    })?;
                Ok(vec![NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: index,
                    arguments_delta: partial_json.to_string(),
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn decode_content_block_stop(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_stop")?;
        let index = Self::required_index(value, "content_block_stop")?;
        let block_kind = self.open_blocks.remove(&index).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic content_block_stop references unopened content block {index}"
            ))
        })?;

        if block_kind == AnthropicContentBlockKind::ToolUse {
            Ok(vec![NeutralStreamEvent::ToolCallEnd {
                choice_index: 0,
                tool_call_index: index,
            }])
        } else {
            Ok(Vec::new())
        }
    }

    fn decode_message_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("message_delta")?;
        let usage = Self::parse_usage(value.get("usage"), &self.usage, "message_delta")?;
        let delta = Self::required_object(value, "delta", "message_delta")?;
        let stop_reason = match delta.get("stop_reason") {
            None | Some(Value::Null) => None,
            Some(Value::String(reason)) => Some(reason.clone()),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_delta delta.stop_reason must be a string",
                ));
            }
        };

        let mut events = Vec::new();
        if let Some(usage) = usage {
            self.usage = usage.clone();
            events.push(NeutralStreamEvent::UsageUpdate(usage));
        }
        if let Some(raw_finish_reason) = stop_reason {
            if !self.finish_emitted {
                self.finish_emitted = true;
                events.push(NeutralStreamEvent::Finish {
                    choice_index: 0,
                    reason: AnthropicAdapter::normalize_finish_reason(&raw_finish_reason),
                    raw_finish_reason: Some(raw_finish_reason),
                });
            }
        }
        Ok(events)
    }

    fn close_response(&mut self) -> Vec<NeutralStreamEvent> {
        if self.response_ended {
            return Vec::new();
        }

        let mut events = self
            .open_blocks
            .iter()
            .filter_map(|(index, block_kind)| {
                (*block_kind == AnthropicContentBlockKind::ToolUse).then_some(
                    NeutralStreamEvent::ToolCallEnd {
                        choice_index: 0,
                        tool_call_index: *index,
                    },
                )
            })
            .collect::<Vec<_>>();
        self.open_blocks.clear();
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd);
        events
    }

    fn decode_message_stop(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        self.ensure_active_message("message_stop")?;
        Ok(self.close_response())
    }
}

impl ProviderStreamDecoder for AnthropicStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        let value: Value = serde_json::from_str(data).map_err(|error| {
            Self::stream_error(format!(
                "Failed to parse Anthropic stream event JSON: {error}"
            ))
        })?;
        let event_type = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            Self::stream_error("Anthropic stream event is missing string field type")
        })?;

        match event_type {
            "message_start" => self.decode_message_start(&value),
            "content_block_start" => self.decode_content_block_start(&value),
            "content_block_delta" => self.decode_content_block_delta(&value),
            "content_block_stop" => self.decode_content_block_stop(&value),
            "message_delta" => self.decode_message_delta(&value),
            "message_stop" => self.decode_message_stop(),
            _ => Ok(Vec::new()),
        }
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(self.close_response())
    }
}

#[derive(Default)]
pub struct AnthropicAdapter;

impl AnthropicAdapter {
    pub fn new() -> Self {
        Self
    }

    fn normalize_finish_reason(reason: &str) -> FinishReason {
        match reason {
            "end_turn" | "stop_sequence" => FinishReason::Stop,
            "max_tokens" => FinishReason::MaxTokens,
            "tool_use" => FinishReason::ToolCall,
            _ => FinishReason::Other,
        }
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

        if let Some(level) = route.final_reasoning_level {
            match route
                .upstream_model
                .capabilities
                .reasoning
                .mapping_for(level)
            {
                Some(ReasoningMapping::BudgetTokens(budget_tokens)) => {
                    payload["thinking"] = json!({
                        "type": "enabled",
                        "budget_tokens": budget_tokens
                    });
                }
                Some(ReasoningMapping::Adaptive) => {
                    payload["thinking"] = json!({ "type": "adaptive" });
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

    fn build_headers(&self, provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
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
        let raw_finish_reason = val["stop_reason"].as_str().map(|s| s.to_string());
        let finish_reason = raw_finish_reason
            .as_deref()
            .map(Self::normalize_finish_reason);

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
            choices: vec![NeutralChoice {
                index: 0,
                blocks,
                finish_reason,
                raw_finish_reason,
            }],
            usage,
        })
    }

    fn create_stream_decoder(
        &self,
        upstream_model: &UpstreamModel,
    ) -> Box<dyn ProviderStreamDecoder> {
        Box::new(AnthropicStreamDecoder::new(upstream_model))
    }
}
