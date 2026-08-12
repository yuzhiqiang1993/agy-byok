use crate::domain::{
    ErrorCategory, FinishReason, NeutralChatResponse, NeutralContentBlock, NeutralStreamEvent,
    ProxyError, UsageInfo,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct AntigravityResponseEncoder;

impl AntigravityResponseEncoder {
    fn finish_reason_value(reason: FinishReason) -> &'static str {
        match reason {
            FinishReason::Stop => "STOP",
            FinishReason::MaxTokens => "MAX_TOKENS",
            FinishReason::ToolCall => "TOOL_CALL",
            FinishReason::ContentFilter => "SAFETY",
            FinishReason::Other => "OTHER",
        }
    }

    fn encode_usage_metadata(usage: &UsageInfo) -> Value {
        let mut metadata = json!({
            "promptTokenCount": usage.prompt_tokens(),
            "candidatesTokenCount": usage.output_tokens,
            "totalTokenCount": usage.total_tokens
        });
        if let Some(tokens) = usage.cache_read_tokens {
            metadata["cachedContentTokenCount"] = json!(tokens);
        }
        if let Some(tokens) = usage.reasoning_tokens {
            metadata["thoughtsTokenCount"] = json!(tokens);
        }
        metadata
    }

    fn encode_blocks(blocks: &[NeutralContentBlock]) -> Vec<Value> {
        let mut parts = Vec::new();
        for block in blocks {
            match block {
                NeutralContentBlock::Text(text) => {
                    parts.push(json!({ "text": text }));
                }
                NeutralContentBlock::InlineData {
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
                NeutralContentBlock::Thinking { text, signature } => {
                    let mut part = json!({ "thought": true, "text": text });
                    if let Some(signature) = signature {
                        part["thoughtSignature"] = json!(signature);
                    }
                    parts.push(part);
                }
                NeutralContentBlock::ToolCall {
                    id,
                    name,
                    arguments_json,
                } => {
                    let args = serde_json::from_str(arguments_json).unwrap_or(json!({}));
                    parts.push(json!({
                        "functionCall": {
                            "id": id,
                            "name": name,
                            "args": args
                        }
                    }));
                }
                _ => {}
            }
        }
        parts
    }

    pub fn encode_response(resp: &NeutralChatResponse) -> String {
        let candidates: Vec<Value> = resp
            .choices
            .iter()
            .map(|choice| {
                let mut candidate = json!({
                    "index": choice.index,
                    "content": {
                        "role": "model",
                        "parts": Self::encode_blocks(&choice.blocks)
                    }
                });

                if let Some(reason) = choice.finish_reason {
                    candidate["finishReason"] = json!(Self::finish_reason_value(reason));
                }
                candidate
            })
            .collect();

        let mut payload = json!({
            "candidates": candidates,
            "modelVersion": resp.model,
        });
        if let Some(ref usage) = resp.usage {
            payload["usageMetadata"] = Self::encode_usage_metadata(usage);
        }

        payload.to_string()
    }
}

#[derive(Debug)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug)]
struct PendingFinish {
    choice_index: u32,
    reason: FinishReason,
}

#[derive(Debug, Default)]
pub struct AntigravityStreamEncoder {
    pending_tool_calls: HashMap<(u32, u32), PendingToolCall>,
    pending_finishes: Vec<PendingFinish>,
    model_version: Option<String>,
    response_ended: bool,
}

impl AntigravityStreamEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 先写入配置模型作为兜底；上游开始事件返回非空模型时会自动替换。
    pub fn with_model_version(mut self, model_version: impl Into<String>) -> Self {
        let model_version = model_version.into();
        self.model_version = (!model_version.trim().is_empty()).then_some(model_version);
        self
    }

    pub fn encode_event(&mut self, event: &NeutralStreamEvent) -> Result<Vec<String>, ProxyError> {
        if self.response_ended {
            return match event {
                NeutralStreamEvent::ResponseEnd { .. } => Ok(Vec::new()),
                _ => Err(Self::stream_error(
                    "Received stream event after response end",
                )),
            };
        }

        match event {
            NeutralStreamEvent::ResponseStart { model, .. } => {
                if !model.trim().is_empty() {
                    self.model_version = Some(model.clone());
                }
                Ok(Vec::new())
            }
            NeutralStreamEvent::TextDelta { choice_index, text } => Ok(vec![self.sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "text": text }]
                    }
                }]
            }))]),
            NeutralStreamEvent::InlineData {
                choice_index,
                mime_type,
                data_base64,
            } => Ok(vec![self.sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{
                            "inlineData": {
                                "mimeType": mime_type,
                                "data": data_base64
                            }
                        }]
                    }
                }]
            }))]),
            NeutralStreamEvent::ThinkingDelta { choice_index, text } => Ok(vec![self.sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "thought": true, "text": text }]
                    }
                }]
            }))]),
            NeutralStreamEvent::ThinkingSignature {
                choice_index,
                signature,
            } => Ok(vec![self.sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "thought": true, "thoughtSignature": signature }]
                    }
                }]
            }))]),
            NeutralStreamEvent::ToolCallStart {
                choice_index,
                tool_call_index,
                id,
                name,
            } => {
                let key = (*choice_index, *tool_call_index);
                if self.pending_tool_calls.contains_key(&key) {
                    return Err(Self::stream_error(format!(
                        "Tool call choice {} index {} started more than once",
                        choice_index, tool_call_index
                    )));
                }
                self.pending_tool_calls.insert(
                    key,
                    PendingToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    },
                );
                Ok(Vec::new())
            }
            NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index,
                tool_call_index,
                arguments_delta,
            } => {
                let pending = self
                    .pending_tool_calls
                    .get_mut(&(*choice_index, *tool_call_index))
                    .ok_or_else(|| {
                        Self::stream_error(format!(
                            "Tool arguments reference unopened choice {} index {}",
                            choice_index, tool_call_index
                        ))
                    })?;
                pending.arguments.push_str(arguments_delta);
                Ok(Vec::new())
            }
            NeutralStreamEvent::ToolCallEnd {
                choice_index,
                tool_call_index,
            } => {
                let pending = self
                    .pending_tool_calls
                    .remove(&(*choice_index, *tool_call_index))
                    .ok_or_else(|| {
                        Self::stream_error(format!(
                            "Tool end references unopened choice {} index {}",
                            choice_index, tool_call_index
                        ))
                    })?;
                let arguments = if pending.arguments.is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&pending.arguments).map_err(|error| {
                        Self::stream_error(format!(
                            "Invalid tool arguments JSON for choice {} index {}: {}",
                            choice_index, tool_call_index, error
                        ))
                    })?
                };
                Ok(vec![self.sse(json!({
                    "candidates": [{
                        "index": choice_index,
                        "content": {
                            "role": "model",
                            "parts": [{
                                "functionCall": {
                                    "id": pending.id,
                                    "name": pending.name,
                                    "args": arguments
                                }
                            }]
                        }
                    }]
                }))])
            }
            NeutralStreamEvent::Finish {
                choice_index,
                reason,
                ..
            } => {
                self.pending_finishes.push(PendingFinish {
                    choice_index: *choice_index,
                    reason: *reason,
                });
                Ok(Vec::new())
            }
            NeutralStreamEvent::ResponseEnd { usage } => {
                if !self.pending_tool_calls.is_empty() {
                    return Err(Self::stream_error(
                        "Response ended while tool calls were still open",
                    ));
                }

                let mut frames = self
                    .take_final_frame(usage.as_ref(), usage.is_some())
                    .into_iter()
                    .collect::<Vec<_>>();
                frames.push("data: [DONE]\n\n".to_string());
                self.response_ended = true;
                Ok(frames)
            }
            NeutralStreamEvent::Error { message, code } => Ok(vec![self.sse(json!({
                "error": {
                    "code": code,
                    "message": message
                }
            }))]),
        }
    }

    pub fn abort(&mut self) -> Vec<String> {
        if self.response_ended || !self.pending_tool_calls.is_empty() {
            return Vec::new();
        }
        self.response_ended = true;
        self.take_final_frame(None, false).into_iter().collect()
    }

    fn take_final_frame(
        &mut self,
        usage: Option<&UsageInfo>,
        synthesize_candidate: bool,
    ) -> Option<String> {
        let mut candidates = self
            .pending_finishes
            .drain(..)
            .map(|finish| {
                json!({
                    "index": finish.choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "text": "" }]
                    },
                    "finishReason": AntigravityResponseEncoder::finish_reason_value(finish.reason)
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() && synthesize_candidate {
            candidates.push(json!({
                "index": 0,
                "content": {
                    "role": "model",
                    "parts": [{ "text": "" }]
                },
                "finishReason": AntigravityResponseEncoder::finish_reason_value(
                    FinishReason::Other,
                )
            }));
        }
        if candidates.is_empty() {
            return None;
        }

        let mut payload = json!({ "candidates": candidates });
        if let Some(usage) = usage {
            payload["usageMetadata"] = AntigravityResponseEncoder::encode_usage_metadata(usage);
        }
        Some(self.sse(payload))
    }

    fn sse(&self, mut payload: Value) -> String {
        if let Some(model_version) = &self.model_version {
            payload["modelVersion"] = json!(model_version);
        }
        format!("data: {}\n\n", payload)
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }
}
