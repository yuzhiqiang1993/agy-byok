use super::normalize_finish_reason;
use crate::domain::response::NeutralChoice;
use crate::domain::{
    ErrorCategory, NeutralChatResponse, NeutralContentBlock, ProxyError, UpstreamModel, UsageInfo,
};
use crate::providers::error::classify_response_error;
use serde_json::Value;

pub(super) fn parse_response(
    status: u16,
    body: &str,
    upstream_model: &UpstreamModel,
) -> Result<NeutralChatResponse, ProxyError> {
    if status >= 400 {
        let cat = classify_response_error(status, body);
        return Err(
            ProxyError::new(cat, format!("Anthropic upstream status {}", status), status)
                .with_upstream_body(body),
        );
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
    let finish_reason = raw_finish_reason.as_deref().map(normalize_finish_reason);

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

    let usage = val["usage"].as_object().map(|usage| {
        let token = |field: &str| {
            usage
                .get(field)
                .and_then(Value::as_u64)
                .and_then(|tokens| u32::try_from(tokens).ok())
        };
        let input_tokens = token("input_tokens").unwrap_or(0);
        let output_tokens = token("output_tokens").unwrap_or(0);
        let cache_read_tokens = token("cache_read_input_tokens");
        let cache_write_tokens = token("cache_creation_input_tokens");
        let total_tokens = input_tokens
            .saturating_add(output_tokens)
            .saturating_add(cache_read_tokens.unwrap_or(0))
            .saturating_add(cache_write_tokens.unwrap_or(0));

        UsageInfo {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens: None,
            total_tokens,
        }
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
