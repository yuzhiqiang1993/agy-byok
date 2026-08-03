use super::normalize_finish_reason;
use crate::domain::response::NeutralChoice;
use crate::domain::{
    ErrorCategory, NeutralChatResponse, NeutralContentBlock, ProxyError, UpstreamModel, UsageInfo,
};
use serde_json::Value;

pub(super) fn parse_response(
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
            let finish_reason = raw_finish_reason.as_deref().map(normalize_finish_reason);
            let mut blocks = Vec::new();

            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for (part_position, part) in parts.iter().enumerate() {
                    if part
                        .get("thought")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        let text = part["text"].as_str().unwrap_or_default();
                        let signature = part["thoughtSignature"].as_str().map(str::to_string);
                        if !text.is_empty() || signature.is_some() {
                            blocks.push(NeutralContentBlock::Thinking {
                                text: text.to_string(),
                                signature,
                            });
                        }
                    } else if let Some(text) = part["text"].as_str() {
                        blocks.push(NeutralContentBlock::Text(text.to_string()));
                    } else if let Some(function_call) = part.get("functionCall") {
                        let name = function_call["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let arguments_json = function_call["args"].to_string();
                        let id = function_call["id"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                format!("call_{}_{}", candidate_position, part_position)
                            });
                        blocks.push(NeutralContentBlock::ToolCall {
                            id,
                            name,
                            arguments_json,
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

    let usage = val["usageMetadata"].as_object().map(|usage| UsageInfo {
        prompt_tokens: usage
            .get("promptTokenCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: usage
            .get("candidatesTokenCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: usage
            .get("totalTokenCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
    });

    Ok(NeutralChatResponse {
        id,
        model,
        choices,
        usage,
    })
}
