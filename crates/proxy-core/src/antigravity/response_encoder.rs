use crate::domain::{FinishReason, NeutralChatResponse, NeutralContentBlock, NeutralStreamEvent};
use serde_json::{json, Value};

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
                NeutralContentBlock::Thinking { text, .. } => {
                    parts.push(json!({ "thought": true, "text": text }));
                }
                NeutralContentBlock::ToolCall {
                    name,
                    arguments_json,
                    ..
                } => {
                    let args = serde_json::from_str(arguments_json).unwrap_or(json!({}));
                    parts.push(json!({
                        "functionCall": {
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

    pub fn encode_stream_event(event: &NeutralStreamEvent) -> Option<String> {
        match event {
            NeutralStreamEvent::TextDelta { choice_index, text } => {
                let payload = json!({
                    "candidates": [{
                        "index": choice_index,
                        "content": {
                            "role": "model",
                            "parts": [{ "text": text }]
                        }
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::ThinkingDelta { choice_index, text } => {
                let payload = json!({
                    "candidates": [{
                        "index": choice_index,
                        "content": {
                            "role": "model",
                            "parts": [{ "thought": true, "text": text }]
                        }
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::ToolCallDelta { .. } => None,
            NeutralStreamEvent::UsageUpdate(usage) => {
                let payload = json!({
                    "usageMetadata": {
                        "promptTokenCount": usage.prompt_tokens,
                        "candidatesTokenCount": usage.completion_tokens,
                        "totalTokenCount": usage.total_tokens
                    }
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::Finish {
                choice_index,
                reason,
                ..
            } => {
                let payload = json!({
                    "candidates": [{
                        "index": choice_index,
                        "finishReason": Self::finish_reason_value(*reason)
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::Error { message, code } => {
                let payload = json!({
                    "error": {
                        "code": code,
                        "message": message
                    }
                });
                Some(format!("data: {}\n\n", payload))
            }
        }
    }
}
