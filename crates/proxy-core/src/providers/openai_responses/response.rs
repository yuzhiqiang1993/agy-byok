use super::{normalize_finish_reason, parse_usage};
use crate::domain::response::{FinishReason, NeutralChoice};
use crate::domain::{
    ErrorCategory, NeutralChatResponse, NeutralContentBlock, ProxyError, UpstreamModel,
};
use serde_json::Value;

fn parse_output_blocks(output: &Value) -> Vec<NeutralContentBlock> {
    let mut blocks = Vec::new();
    let Some(items) = output.as_array() else {
        return blocks;
    };

    for item in items {
        match item["type"].as_str().unwrap_or_default() {
            "message" => {
                if let Some(content) = item["content"].as_array() {
                    for part in content {
                        match part["type"].as_str().unwrap_or_default() {
                            "output_text" => {
                                if let Some(text) = part["text"].as_str() {
                                    if !text.is_empty() {
                                        blocks.push(NeutralContentBlock::Text(text.to_string()));
                                    }
                                }
                            }
                            "refusal" => {
                                if let Some(refusal) = part["refusal"].as_str() {
                                    if !refusal.is_empty() {
                                        blocks.push(NeutralContentBlock::Text(refusal.to_string()));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "reasoning" => {
                if let Some(summary) = item["summary"].as_array() {
                    for part in summary {
                        if let Some(text) = part["text"].as_str() {
                            blocks.push(NeutralContentBlock::Thinking {
                                text: text.to_string(),
                                signature: None,
                            });
                        }
                    }
                }
            }
            "function_call" => {
                let id = item["call_id"]
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = item["name"].as_str().unwrap_or_default().to_string();
                let arguments_json = item["arguments"].as_str().unwrap_or("{}").to_string();
                blocks.push(NeutralContentBlock::ToolCall {
                    id,
                    name,
                    arguments_json,
                });
            }
            _ => {}
        }
    }
    blocks
}

fn parse_error(status: u16, body: &str) -> ProxyError {
    let category = match status {
        401 | 403 => ErrorCategory::Authentication,
        404 => ErrorCategory::ModelNotFound,
        429 => ErrorCategory::RateLimit,
        500..=599 => ErrorCategory::UpstreamServerError,
        _ => ErrorCategory::InvalidRequest,
    };
    ProxyError::new(
        category,
        format!("OpenAI Responses upstream status {status}"),
        status,
    )
    .with_upstream_body(body)
}

pub(super) fn parse_response(
    status: u16,
    body: &str,
    upstream_model: &UpstreamModel,
) -> Result<NeutralChatResponse, ProxyError> {
    if status >= 400 {
        return Err(parse_error(status, body));
    }
    let value: Value = serde_json::from_str(body).map_err(|error| {
        ProxyError::new(
            ErrorCategory::Internal,
            format!("Failed to parse OpenAI Responses JSON response: {error}"),
            500,
        )
    })?;
    let blocks = parse_output_blocks(&value["output"]);
    let status = value["status"].as_str().unwrap_or("completed");
    let raw_finish_reason = value["incomplete_details"]["reason"]
        .as_str()
        .or(Some(status))
        .map(str::to_string);
    let finish_reason = if blocks
        .iter()
        .any(|block| matches!(block, NeutralContentBlock::ToolCall { .. }))
    {
        Some(FinishReason::ToolCall)
    } else {
        raw_finish_reason.as_deref().map(normalize_finish_reason)
    };

    Ok(NeutralChatResponse {
        id: value["id"].as_str().unwrap_or("resp-id").to_string(),
        model: value["model"]
            .as_str()
            .unwrap_or(&upstream_model.upstream_model_id)
            .to_string(),
        choices: vec![NeutralChoice {
            index: 0,
            blocks,
            finish_reason,
            raw_finish_reason,
        }],
        usage: parse_usage(&value["usage"]),
    })
}
