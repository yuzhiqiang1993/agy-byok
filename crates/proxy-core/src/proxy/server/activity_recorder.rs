use super::ProxyServer;
use crate::domain::{NeutralChatRequest, ProviderProtocol, ProxyError, UsageInfo};
use crate::proxy::activity::ActivityItem;
use crate::routing::ResolvedRoute;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct ActivityOutcome<'a> {
    status_code: u16,
    duration_ms: u64,
    fallback_attempted: bool,
    fallback_succeeded: bool,
    usage: Option<&'a UsageInfo>,
    error: Option<&'a ProxyError>,
}

impl<'a> ActivityOutcome<'a> {
    pub(super) fn success(
        duration_ms: u64,
        used_fallback: bool,
        usage: Option<&'a UsageInfo>,
    ) -> Self {
        Self {
            status_code: 200,
            duration_ms,
            fallback_attempted: used_fallback,
            fallback_succeeded: used_fallback,
            usage,
            error: None,
        }
    }

    pub(super) fn failure(
        duration_ms: u64,
        fallback_attempted: bool,
        error: &'a ProxyError,
    ) -> Self {
        Self {
            status_code: error.status_code,
            duration_ms,
            fallback_attempted,
            fallback_succeeded: false,
            usage: None,
            error: Some(error),
        }
    }
}

impl ProxyServer {
    pub fn record_official_generation(
        &self,
        model_id: &str,
        stream: bool,
        message_count: usize,
        tool_count: usize,
        status_code: u16,
        duration_ms: u64,
    ) {
        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "chat".to_string(),
            operation: if stream {
                "stream_generate".to_string()
            } else {
                "generate".to_string()
            },
            request_method: "POST".to_string(),
            request_path: if stream {
                "/v1internal:streamGenerateContent".to_string()
            } else {
                "/v1internal:generateContent".to_string()
            },
            request_body_bytes: None,
            response_body_bytes: None,
            response_summary: None,
            timestamp_ms: Self::current_time_ms(),
            requested_virtual_model_id: model_id.to_string(),
            virtual_model_id: model_id.to_string(),
            upstream_model_id: Some(model_id.to_string()),
            provider_id: "antigravity-official".to_string(),
            provider_protocol: Some("native".to_string()),
            status_code,
            duration_ms,
            error_category: (!matches!(status_code, 200..=299))
                .then(|| "OfficialUpstream".to_string()),
            error_detail: None,
            stream,
            message_count,
            tool_count,
            used_fallback: false,
            fallback_attempted: false,
            fallback_succeeded: false,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        });
    }

    pub(super) fn record_activity(
        &self,
        route: Option<&ResolvedRoute>,
        request: &NeutralChatRequest,
        outcome: ActivityOutcome<'_>,
    ) {
        let now_ms = Self::current_time_ms();
        let (virtual_model_id, upstream_model_id, provider_id, provider_protocol) = match route {
            Some(route) => (
                route.virtual_model.id.clone(),
                Some(route.upstream_model.upstream_model_id.clone()),
                route.provider.id.clone(),
                Some(match route.provider.protocol {
                    ProviderProtocol::OpenaiChatCompletions => {
                        "openai_chat_completions".to_string()
                    }
                    ProviderProtocol::AnthropicMessages => "anthropic_messages".to_string(),
                    ProviderProtocol::GeminiGenerateContent => {
                        "gemini_generate_content".to_string()
                    }
                    ProviderProtocol::OpenaiResponses => "openai_responses".to_string(),
                }),
            ),
            None => (
                request.virtual_model_id.clone(),
                None,
                "unknown".to_string(),
                None,
            ),
        };

        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "chat".to_string(),
            operation: if request.stream {
                "stream_generate".to_string()
            } else {
                "generate".to_string()
            },
            request_method: "POST".to_string(),
            request_path: if request.stream {
                "/v1internal:streamGenerateContent".to_string()
            } else {
                "/v1internal:generateContent".to_string()
            },
            request_body_bytes: None,
            response_body_bytes: None,
            response_summary: None,
            timestamp_ms: now_ms,
            requested_virtual_model_id: request.virtual_model_id.clone(),
            virtual_model_id,
            upstream_model_id,
            provider_id,
            provider_protocol,
            status_code: outcome.status_code,
            duration_ms: outcome.duration_ms,
            error_category: outcome.error.map(|error| format!("{:?}", error.category)),
            error_detail: outcome.error.map(|error| {
                Self::sanitized_upstream_error(error)
                    .unwrap_or_else(|| Self::sanitize_log_text(&error.message))
            }),
            stream: request.stream,
            message_count: request.messages.len(),
            tool_count: request.tools.len(),
            used_fallback: outcome.fallback_succeeded,
            fallback_attempted: outcome.fallback_attempted,
            fallback_succeeded: outcome.fallback_succeeded,
            input_tokens: outcome.usage.map(|usage| usage.input_tokens),
            output_tokens: outcome.usage.map(|usage| usage.output_tokens),
            cache_read_tokens: outcome.usage.and_then(|usage| usage.cache_read_tokens),
            cache_write_tokens: outcome.usage.and_then(|usage| usage.cache_write_tokens),
            reasoning_tokens: outcome.usage.and_then(|usage| usage.reasoning_tokens),
            total_tokens: outcome.usage.map(|usage| usage.total_tokens),
        });
    }

    pub fn record_http_activity(
        &self,
        operation: &str,
        request_method: &str,
        request_path: &str,
        request_body_bytes: Option<u64>,
        status_code: u16,
        duration_ms: u64,
        response_body_bytes: Option<u64>,
        response_summary: Option<&str>,
        error_category: Option<&str>,
        error_detail: Option<&str>,
    ) {
        self.activity_log.record(ActivityItem {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "http".to_string(),
            operation: operation.to_string(),
            request_method: request_method.to_string(),
            request_path: Self::sanitize_log_text(request_path),
            request_body_bytes,
            response_body_bytes,
            response_summary: response_summary.map(Self::sanitize_log_text),
            timestamp_ms: Self::current_time_ms(),
            requested_virtual_model_id: request_path.to_string(),
            virtual_model_id: request_path.to_string(),
            upstream_model_id: None,
            provider_id: "local-proxy".to_string(),
            provider_protocol: Some("http".to_string()),
            status_code,
            duration_ms,
            error_category: error_category.map(Self::sanitize_log_text),
            error_detail: error_detail.map(Self::sanitize_log_text),
            stream: false,
            message_count: 0,
            tool_count: 0,
            used_fallback: false,
            fallback_attempted: false,
            fallback_succeeded: false,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        });
    }

    pub(super) fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    pub(super) fn sanitized_upstream_error(error: &ProxyError) -> Option<String> {
        let body = error.upstream_body.as_deref()?;
        let payload: serde_json::Value = serde_json::from_str(body).ok()?;
        let detail = payload.get("error").unwrap_or(&payload);
        if let Some(message) = detail.as_str() {
            return Some(Self::sanitize_log_text(message));
        }

        let object = detail.as_object()?;
        let fields = ["message", "type", "param", "code"];
        let parts = fields
            .into_iter()
            .filter_map(|key| {
                let value = object.get(key)?;
                let raw = match value {
                    serde_json::Value::String(value) => value.clone(),
                    serde_json::Value::Number(value) => value.to_string(),
                    serde_json::Value::Bool(value) => value.to_string(),
                    _ => return None,
                };
                Some(format!("{key}={}", Self::sanitize_log_text(&raw)))
            })
            .collect::<Vec<_>>();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    pub(super) fn sanitize_log_text(value: &str) -> String {
        let mut redact_next = false;
        let mut sanitized = Vec::new();
        for token in value.split_whitespace() {
            let comparable = token
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '=')
                })
                .to_ascii_lowercase();
            if redact_next {
                sanitized.push("[REDACTED]".to_string());
                redact_next = false;
            } else if comparable == "bearer" {
                sanitized.push("Bearer".to_string());
                redact_next = true;
            } else if comparable.starts_with("sk-")
                || comparable.starts_with("api_key=")
                || comparable.starts_with("apikey=")
                || comparable.starts_with("authorization=")
            {
                sanitized.push("[REDACTED]".to_string());
            } else {
                sanitized.push(token.to_string());
            }
        }
        sanitized.join(" ").chars().take(500).collect()
    }
}
