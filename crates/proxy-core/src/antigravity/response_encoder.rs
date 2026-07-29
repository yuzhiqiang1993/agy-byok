use crate::domain::{NeutralChatResponse, NeutralContentBlock, NeutralStreamEvent};
use serde_json::json;

pub struct AntigravityResponseEncoder;

impl AntigravityResponseEncoder {
    pub fn encode_response(resp: &NeutralChatResponse) -> String {
        let mut parts = Vec::new();
        for block in &resp.choices_blocks {
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
                    let args: serde_json::Value =
                        serde_json::from_str(arguments_json).unwrap_or(json!({}));
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

        let mut candidate = json!({
            "content": {
                "role": "model",
                "parts": parts
            }
        });

        if let Some(ref reason) = resp.finish_reason {
            candidate["finishReason"] = json!(reason);
        }

        let mut payload = json!({
            "candidates": [candidate]
        });

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
            NeutralStreamEvent::TextDelta(text) => {
                let payload = json!({
                    "candidates": [{
                        "content": {
                            "role": "model",
                            "parts": [{ "text": text }]
                        }
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::ThinkingDelta(text) => {
                let payload = json!({
                    "candidates": [{
                        "content": {
                            "role": "model",
                            "parts": [{ "thought": true, "text": text }]
                        }
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
            NeutralStreamEvent::ToolCallDelta {
                name,
                arguments_delta,
                ..
            } => {
                let args_val: serde_json::Value = serde_json::from_str(arguments_delta)
                    .unwrap_or_else(|_| json!({ "delta": arguments_delta }));
                let payload = json!({
                    "candidates": [{
                        "content": {
                            "role": "model",
                            "parts": [{
                                "functionCall": {
                                    "name": name.as_deref().unwrap_or("tool"),
                                    "args": args_val
                                }
                            }]
                        }
                    }]
                });
                Some(format!("data: {}\n\n", payload))
            }
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
            NeutralStreamEvent::Finish { reason } => {
                let payload = json!({
                    "candidates": [{
                        "finishReason": reason
                    }]
                });
                Some(format!("data: {}\n\ndata: [DONE]\n\n", payload))
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
