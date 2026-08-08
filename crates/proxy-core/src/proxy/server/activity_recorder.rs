use super::ProxyServer;
use crate::domain::{NeutralChatRequest, ProxyError, UsageInfo};
use crate::proxy::activity::{
    ActivityCommon, ActivityErrorCategory, ActivityItem, ActivityOperation, ActivityProtocol,
    ChatActivityItem, HttpActivityItem,
};
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

/// HTTP 请求结束后写入活动日志所需的完整快照。
pub(crate) struct HttpActivity<'a> {
    pub operation: ActivityOperation,
    pub request_method: &'a str,
    pub request_path: &'a str,
    pub request_body_bytes: Option<u64>,
    pub status_code: u16,
    pub duration_ms: u64,
    pub response_body_bytes: Option<u64>,
    pub response_summary: Option<&'a str>,
    pub error_category: Option<ActivityErrorCategory>,
    pub error_detail: Option<&'a str>,
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
    pub(crate) fn record_official_generation(
        &self,
        model_id: &str,
        stream: bool,
        message_count: usize,
        tool_count: usize,
        status_code: u16,
        duration_ms: u64,
    ) {
        self.activity_log
            .record(ActivityItem::chat(ChatActivityItem {
                common: ActivityCommon {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp_ms: Self::current_time_ms(),
                    status_code,
                    duration_ms,
                    error_category: (!matches!(status_code, 200..=299))
                        .then_some(ActivityErrorCategory::OfficialUpstream),
                    error_detail: None,
                },
                requested_virtual_model_id: model_id.to_string(),
                virtual_model_id: model_id.to_string(),
                upstream_model_id: Some(model_id.to_string()),
                provider_id: "antigravity-official".to_string(),
                provider_protocol: Some(ActivityProtocol::Native),
                stream,
                message_count,
                tool_count,
                fallback_attempted: false,
                fallback_succeeded: false,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                total_tokens: None,
            }));
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
                Some(ActivityProtocol::from(&route.provider.protocol)),
            ),
            None => (
                request.virtual_model_id.clone(),
                None,
                "unknown".to_string(),
                None,
            ),
        };

        self.activity_log
            .record(ActivityItem::chat(ChatActivityItem {
                common: ActivityCommon {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp_ms: now_ms,
                    status_code: outcome.status_code,
                    duration_ms: outcome.duration_ms,
                    error_category: outcome
                        .error
                        .map(|error| ActivityErrorCategory::from(&error.category)),
                    error_detail: None,
                },
                requested_virtual_model_id: request.virtual_model_id.clone(),
                virtual_model_id,
                upstream_model_id,
                provider_id,
                provider_protocol,
                stream: request.stream,
                message_count: request.messages.len(),
                tool_count: request.tools.len(),
                fallback_attempted: outcome.fallback_attempted,
                fallback_succeeded: outcome.fallback_succeeded,
                input_tokens: outcome.usage.map(|usage| usage.input_tokens),
                output_tokens: outcome.usage.map(|usage| usage.output_tokens),
                cache_read_tokens: outcome.usage.and_then(|usage| usage.cache_read_tokens),
                cache_write_tokens: outcome.usage.and_then(|usage| usage.cache_write_tokens),
                reasoning_tokens: outcome.usage.and_then(|usage| usage.reasoning_tokens),
                total_tokens: outcome.usage.map(|usage| usage.total_tokens),
            }));
    }

    pub(crate) fn record_http_activity(&self, activity: HttpActivity<'_>) {
        self.activity_log
            .record(ActivityItem::http(HttpActivityItem {
                common: ActivityCommon {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp_ms: Self::current_time_ms(),
                    status_code: activity.status_code,
                    duration_ms: activity.duration_ms,
                    error_category: activity.error_category,
                    error_detail: activity.error_detail.map(Self::sanitize_log_text),
                },
                operation: activity.operation,
                request_method: activity.request_method.to_string(),
                request_path: Self::sanitize_log_text(activity.request_path),
                request_body_bytes: activity.request_body_bytes,
                response_body_bytes: activity.response_body_bytes,
                response_summary: activity.response_summary.map(Self::sanitize_log_text),
            }));
    }

    pub(super) fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
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
