use crate::domain::ErrorCategory;
use serde_json::Value;

pub(super) fn classify_response_error(status: u16, body: &str) -> ErrorCategory {
    match status {
        401 | 403 => ErrorCategory::Authentication,
        404 => ErrorCategory::ModelNotFound,
        429 => ErrorCategory::RateLimit,
        500..=599 => ErrorCategory::UpstreamServerError,
        _ if has_context_length_error(body) => ErrorCategory::ContextLengthExceeded,
        _ => ErrorCategory::InvalidRequest,
    }
}

pub(super) fn upstream_error_message(provider: &str, status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|payload| {
            let error = payload.get("error").unwrap_or(&payload);
            structured_message(error, &payload)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned)
        });

    match detail {
        Some(detail) => {
            let detail = if detail.chars().count() > 500 {
                let truncated: String = detail.chars().take(500).collect();
                format!("{truncated}...")
            } else {
                detail
            };
            format!("{provider} upstream status {status}: {detail}")
        }
        None => format!("{provider} upstream status {status}"),
    }
}

fn has_context_length_error(body: &str) -> bool {
    let Ok(payload) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let error = payload.get("error").unwrap_or(&payload);

    [error, &payload]
        .into_iter()
        .any(has_context_length_identifier)
        || structured_message(error, &payload).is_some_and(is_context_length_message)
}

fn has_context_length_identifier(value: &Value) -> bool {
    ["code", "type", "status"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .any(is_context_length_identifier)
}

fn is_context_length_identifier(identifier: &str) -> bool {
    let normalized = identifier
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");
    let identifies_context_limit = ["context_length", "context_window", "context_limit"]
        .into_iter()
        .any(|term| normalized.contains(term));
    let identifies_output_limit = [
        "output_token",
        "completion_token",
        "max_token",
        "thinking_token",
        "reasoning_token",
    ]
    .into_iter()
    .any(|term| normalized.contains(term));
    let identifies_token_limit = normalized.contains("token") && !identifies_output_limit;
    let identifies_rate_or_quota = ["rate_limit", "quota", "per_minute", "per_second"]
        .into_iter()
        .any(|term| normalized.contains(term));
    let identifies_excess = ["exceed", "too_long", "too_many", "too_large", "overflow"]
        .into_iter()
        .any(|term| normalized.contains(term));

    !identifies_rate_or_quota
        && identifies_excess
        && (identifies_context_limit || identifies_token_limit)
}

fn structured_message<'a>(error: &'a Value, payload: &'a Value) -> Option<&'a str> {
    error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .or_else(|| error.as_str())
}

fn is_context_length_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    if [
        "rate limit",
        "quota",
        "token per minute",
        "tokens per minute",
        "tokens per second",
    ]
    .into_iter()
    .any(|term| message.contains(term))
    {
        return false;
    }

    let mentions_context_limit = [
        "context length",
        "context window",
        "context limit",
        "context size",
        "maximum context",
        "max context",
    ]
    .into_iter()
    .any(|term| message.contains(term));
    let mentions_token = message.contains("token");
    let mentions_direct_input = ["input", "prompt", "messages"]
        .into_iter()
        .any(|term| message.contains(term));
    let mentions_request_tokens = mentions_token && message.contains("request");
    let mentions_token_limit =
        message.contains("token limit") || message.contains("too many tokens");
    let output_parameter_only = !mentions_context_limit
        && !mentions_direct_input
        && (message.contains("max_tokens")
            || message.contains("max output tokens")
            || message.contains("output token")
            || message.contains("completion token"));
    if output_parameter_only {
        return false;
    }

    let indicates_excess = [
        "exceed",
        "too long",
        "too many",
        "too large",
        "maximum",
        "over the limit",
        "over limit",
        "greater than",
        "more than",
        "must be less than",
        "reached",
        "beyond",
        "at most",
    ]
    .into_iter()
    .any(|term| message.contains(term))
        || message.contains('>')
        || message.contains("<=");

    indicates_excess
        && (mentions_context_limit
            || (mentions_token && mentions_direct_input)
            || mentions_request_tokens
            || mentions_token_limit)
}

#[cfg(test)]
mod tests;
