use crate::domain::{ErrorCategory, ProviderProtocol};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_ACTIVITY_ITEMS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivityOperation {
    HealthCheck,
    ListModels,
    FetchAvailableModels,
    Generate,
    StreamGenerate,
    Passthrough,
    CorsPreflight,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivityProtocol {
    OpenaiChatCompletions,
    AnthropicMessages,
    GeminiGenerateContent,
    OpenaiResponses,
    Native,
}

impl From<&ProviderProtocol> for ActivityProtocol {
    fn from(protocol: &ProviderProtocol) -> Self {
        match protocol {
            ProviderProtocol::OpenaiChatCompletions => Self::OpenaiChatCompletions,
            ProviderProtocol::AnthropicMessages => Self::AnthropicMessages,
            ProviderProtocol::GeminiGenerateContent => Self::GeminiGenerateContent,
            ProviderProtocol::OpenaiResponses => Self::OpenaiResponses,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivityErrorCategory {
    Authentication,
    InvalidRequest,
    ContextLengthExceeded,
    RateLimit,
    ModelNotFound,
    UpstreamServerError,
    Timeout,
    ConnectionFailed,
    StreamInterrupted,
    UnsupportedFeature,
    Internal,
    OfficialUpstream,
    MethodNotAllowed,
    PayloadTooLarge,
    NativeForwardingUnavailable,
    NativeForwardingFailed,
}

impl ActivityErrorCategory {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::InvalidRequest => "invalid_request",
            Self::ContextLengthExceeded => "context_length_exceeded",
            Self::RateLimit => "rate_limit",
            Self::ModelNotFound => "model_not_found",
            Self::UpstreamServerError => "upstream_server_error",
            Self::Timeout => "timeout",
            Self::ConnectionFailed => "connection_failed",
            Self::StreamInterrupted => "stream_interrupted",
            Self::UnsupportedFeature => "unsupported_feature",
            Self::Internal => "internal",
            Self::OfficialUpstream => "official_upstream",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::NativeForwardingUnavailable => "native_forwarding_unavailable",
            Self::NativeForwardingFailed => "native_forwarding_failed",
        }
    }
}

impl From<&ErrorCategory> for ActivityErrorCategory {
    fn from(category: &ErrorCategory) -> Self {
        match category {
            ErrorCategory::Authentication => Self::Authentication,
            ErrorCategory::InvalidRequest => Self::InvalidRequest,
            ErrorCategory::ContextLengthExceeded => Self::ContextLengthExceeded,
            ErrorCategory::RateLimit => Self::RateLimit,
            ErrorCategory::ModelNotFound => Self::ModelNotFound,
            ErrorCategory::UpstreamServerError => Self::UpstreamServerError,
            ErrorCategory::Timeout => Self::Timeout,
            ErrorCategory::ConnectionFailed => Self::ConnectionFailed,
            ErrorCategory::StreamInterrupted => Self::StreamInterrupted,
            ErrorCategory::UnsupportedFeature => Self::UnsupportedFeature,
            ErrorCategory::Internal => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivityCommon {
    pub(crate) id: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) status_code: u16,
    pub(crate) duration_ms: u64,
    pub(crate) error_category: Option<ActivityErrorCategory>,
    pub(crate) error_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatActivityItem {
    #[serde(flatten)]
    pub(crate) common: ActivityCommon,
    pub(crate) requested_virtual_model_id: String,
    pub(crate) virtual_model_id: String,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) provider_id: String,
    pub(crate) provider_protocol: Option<ActivityProtocol>,
    pub(crate) stream: bool,
    pub(crate) message_count: usize,
    pub(crate) tool_count: usize,
    pub(crate) fallback_attempted: bool,
    pub(crate) fallback_succeeded: bool,
    pub(crate) input_tokens: Option<u32>,
    pub(crate) output_tokens: Option<u32>,
    pub(crate) cache_read_tokens: Option<u32>,
    pub(crate) cache_write_tokens: Option<u32>,
    pub(crate) reasoning_tokens: Option<u32>,
    pub(crate) total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpActivityItem {
    #[serde(flatten)]
    pub(crate) common: ActivityCommon,
    pub(crate) operation: ActivityOperation,
    pub(crate) request_method: String,
    pub(crate) request_path: String,
    pub(crate) request_body_bytes: Option<u64>,
    pub(crate) response_body_bytes: Option<u64>,
    pub(crate) response_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ActivityItem(ActivityItemKind);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ActivityItemKind {
    Chat(ChatActivityItem),
    Http(HttpActivityItem),
}

impl ActivityItem {
    pub(crate) fn chat(item: ChatActivityItem) -> Self {
        Self(ActivityItemKind::Chat(item))
    }

    pub(crate) fn http(item: HttpActivityItem) -> Self {
        Self(ActivityItemKind::Http(item))
    }

    #[cfg(test)]
    pub(crate) fn as_chat(&self) -> Option<&ChatActivityItem> {
        match &self.0 {
            ActivityItemKind::Chat(item) => Some(item),
            ActivityItemKind::Http(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn as_http(&self) -> Option<&HttpActivityItem> {
        match &self.0 {
            ActivityItemKind::Chat(_) => None,
            ActivityItemKind::Http(item) => Some(item),
        }
    }
}

pub struct ActivityLog {
    items: Mutex<VecDeque<ActivityItem>>,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityLog {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(MAX_ACTIVITY_ITEMS)),
        }
    }

    pub(crate) fn record(&self, item: ActivityItem) {
        let mut guard = self.items.lock().unwrap();
        if guard.len() >= MAX_ACTIVITY_ITEMS {
            guard.pop_front();
        }
        guard.push_back(item);
    }

    pub fn get_recent(&self) -> Vec<ActivityItem> {
        let guard = self.items.lock().unwrap();
        guard.iter().cloned().collect()
    }

    pub fn clear(&self) {
        let mut guard = self.items.lock().unwrap();
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_activity_serializes_only_chat_fields() {
        let value = serde_json::to_value(ActivityItem::chat(ChatActivityItem {
            common: ActivityCommon {
                id: "activity-1".to_string(),
                timestamp_ms: 1,
                status_code: 200,
                duration_ms: 10,
                error_category: None,
                error_detail: None,
            },
            requested_virtual_model_id: "virtual".to_string(),
            virtual_model_id: "virtual".to_string(),
            upstream_model_id: Some("upstream".to_string()),
            provider_id: "provider".to_string(),
            provider_protocol: Some(ActivityProtocol::OpenaiChatCompletions),
            stream: true,
            message_count: 1,
            tool_count: 0,
            fallback_attempted: false,
            fallback_succeeded: false,
            input_tokens: Some(7),
            output_tokens: Some(5),
            cache_read_tokens: Some(3),
            cache_write_tokens: Some(2),
            reasoning_tokens: Some(4),
            total_tokens: Some(21),
        }))
        .unwrap();

        assert_eq!(value["kind"], "chat");
        assert_eq!(value["providerProtocol"], "openai_chat_completions");
        assert_eq!(value["totalTokens"], 21);
        assert!(value.get("requestBodyBytes").is_none());
        assert!(value.get("usedFallback").is_none());
    }

    #[test]
    fn http_activity_serializes_only_http_fields() {
        let value = serde_json::to_value(ActivityItem::http(HttpActivityItem {
            common: ActivityCommon {
                id: "activity-2".to_string(),
                timestamp_ms: 2,
                status_code: 204,
                duration_ms: 5,
                error_category: None,
                error_detail: None,
            },
            operation: ActivityOperation::HealthCheck,
            request_method: "GET".to_string(),
            request_path: "/health".to_string(),
            request_body_bytes: None,
            response_body_bytes: Some(0),
            response_summary: None,
        }))
        .unwrap();

        assert_eq!(value["kind"], "http");
        assert_eq!(value["operation"], "health_check");
        assert!(value.get("virtualModelId").is_none());
        assert!(value.get("fallbackAttempted").is_none());
    }
}
