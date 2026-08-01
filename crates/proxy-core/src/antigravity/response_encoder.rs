use crate::domain::{
    ErrorCategory, FinishReason, NeutralChatResponse, NeutralContentBlock, NeutralStreamEvent,
    ProxyError,
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

    fn encode_blocks(blocks: &[NeutralContentBlock]) -> Vec<Value> {
        let mut parts = Vec::new();
        for block in blocks {
            match block {
                NeutralContentBlock::Text(text) => {
                    parts.push(json!({ "text": text }));
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

        let mut payload = json!({ "candidates": candidates });
        if let Some(ref usage) = resp.usage {
            payload["usageMetadata"] = json!({
                "promptTokenCount": usage.prompt_tokens,
                "candidatesTokenCount": usage.completion_tokens,
                "totalTokenCount": usage.total_tokens
            });
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

#[derive(Debug, Default)]
pub struct AntigravityStreamEncoder {
    pending_tool_calls: HashMap<(u32, u32), PendingToolCall>,
    response_ended: bool,
}

impl AntigravityStreamEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn encode_event(&mut self, event: &NeutralStreamEvent) -> Result<Vec<String>, ProxyError> {
        if self.response_ended {
            return match event {
                NeutralStreamEvent::ResponseEnd => Ok(Vec::new()),
                _ => Err(Self::stream_error(
                    "Received stream event after response end",
                )),
            };
        }

        match event {
            NeutralStreamEvent::ResponseStart { .. } => Ok(Vec::new()),
            NeutralStreamEvent::TextDelta { choice_index, text } => Ok(vec![Self::sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "text": text }]
                    }
                }]
            }))]),
            NeutralStreamEvent::ThinkingDelta { choice_index, text } => {
                Ok(vec![Self::sse(json!({
                    "candidates": [{
                        "index": choice_index,
                        "content": {
                            "role": "model",
                            "parts": [{ "thought": true, "text": text }]
                        }
                    }]
                }))])
            }
            NeutralStreamEvent::ThinkingSignature {
                choice_index,
                signature,
            } => Ok(vec![Self::sse(json!({
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
                Ok(vec![Self::sse(json!({
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
            NeutralStreamEvent::UsageUpdate(_) => Ok(vec![]),
            NeutralStreamEvent::Finish {
                choice_index,
                reason,
                ..
            } => Ok(vec![Self::sse(json!({
                "candidates": [{
                    "index": choice_index,
                    "content": {
                        "role": "model",
                        "parts": [{ "text": "" }]
                    },
                    "finishReason": AntigravityResponseEncoder::finish_reason_value(*reason)
                }]
            }))]),
            NeutralStreamEvent::ResponseEnd => {
                if !self.pending_tool_calls.is_empty() {
                    return Err(Self::stream_error(
                        "Response ended while tool calls were still open",
                    ));
                }
                self.response_ended = true;
                Ok(vec!["data: [DONE]\n\n".to_string()])
            }
            NeutralStreamEvent::Error { message, code } => Ok(vec![Self::sse(json!({
                "error": {
                    "code": code,
                    "message": message
                }
            }))]),
        }
    }

    fn sse(payload: Value) -> String {
        format!("data: {}\n\n", payload)
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }
}
