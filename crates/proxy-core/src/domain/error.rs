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
