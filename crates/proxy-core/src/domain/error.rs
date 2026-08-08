use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
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
}

impl ErrorCategory {
    /// 跨 Rust、Tauri 与前端边界使用的稳定错误代码。
    pub const fn as_str(&self) -> &'static str {
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyError {
    pub category: ErrorCategory,
    pub message: String,
    pub status_code: u16,
    pub upstream_body: Option<String>,
}

impl ProxyError {
    pub fn new(category: ErrorCategory, message: impl Into<String>, status_code: u16) -> Self {
        Self {
            category,
            message: message.into(),
            status_code,
            upstream_body: None,
        }
    }

    pub fn with_upstream_body(mut self, body: impl Into<String>) -> Self {
        self.upstream_body = Some(body.into());
        self
    }

    pub fn is_retryable_for_fallback(&self) -> bool {
        matches!(
            self.category,
            ErrorCategory::Timeout
                | ErrorCategory::ConnectionFailed
                | ErrorCategory::RateLimit
                | ErrorCategory::UpstreamServerError
        )
    }
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProxyError({:?}, status={}): {}",
            self.category, self.status_code, self.message
        )
    }
}

impl std::error::Error for ProxyError {}
