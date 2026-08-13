use super::{normalize_finish_reason, parse_index, parse_usage};
use crate::domain::response::{FinishReason, NeutralChoice};
use crate::domain::{
    ErrorCategory, NeutralChatResponse, NeutralContentBlock, ProxyError, UpstreamModel,
};
use crate::providers::error::{classify_response_error, upstream_error_message};
use serde_json::Value;

pub(super) fn parse_response(
    status: u16,
    body: &str,
    upstream_model: &UpstreamModel,
) -> Result<NeutralChatResponse, ProxyError> {
    if status >= 400 {
        let cat = classify_response_error(status, body);
        return Err(
            ProxyError::new(cat, upstream_error_message("OpenAI", status, body), status)
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

    // OpenAI images API 响应：`{ "data": [ { "b64_json": "..." } ] }`。
    // 图片生成走独立端点，响应形状与 chat completions 不同，需单独解析。
    if let Some(data) = val["data"].as_array() {
        for (item_position, item) in data.iter().enumerate() {
            let mut blocks = Vec::new();
            if let Some(b64_json) = item["b64_json"].as_str().filter(|s| !s.is_empty()) {
                blocks.push(NeutralContentBlock::InlineData {
                    mime_type: "image/png".to_string(),
                    data_base64: b64_json.to_string(),
                });
            } else if let Some(url) = item["url"].as_str().filter(|s| !s.is_empty()) {
                // 上游只返回 URL（未用 b64_json）：以文本形式回传链接。
                blocks.push(NeutralContentBlock::Text(url.to_string()));
            }
            choices.push(NeutralChoice {
                index: item_position as u32,
                blocks,
                finish_reason: Some(FinishReason::Stop),
                raw_finish_reason: None,
            });
        }
        return Ok(NeutralChatResponse {
            id,
            model,
            choices,
            usage: None,
        });
    }

    if let Some(upstream_choices) = val["choices"].as_array() {
        for (choice_position, choice) in upstream_choices.iter().enumerate() {
            let index = parse_index(choice, choice_position);
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
            let finish_reason = raw_finish_reason.as_deref().map(normalize_finish_reason);
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
        usage: parse_usage(&val["usage"]),
    })
}
