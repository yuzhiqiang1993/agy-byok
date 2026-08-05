use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Frame;
use std::convert::Infallible;
use std::time::Duration;

pub(super) const LOCAL_TOKEN_HEADER: &str = "x-agy-byok-token";
pub(super) const INTERNAL_PROBE_HEADER: &str = "x-agy-byok-internal-probe";

pub(super) type HttpBody = BoxBody<Bytes, Infallible>;
pub(super) type HttpResponse = hyper::Response<HttpBody>;
pub(super) type HttpFrame = Result<Frame<Bytes>, Infallible>;

#[derive(Debug, Clone, Default)]
pub(super) struct HttpActivityMetadata {
    pub(super) response_summary: Option<String>,
    pub(super) error_category: Option<String>,
    pub(super) error_detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub require_auth: bool,
    pub require_host_auth: bool,
    pub fallback_to_random_port_on_bind_error: bool,
    pub max_body_bytes: usize,
    pub max_concurrent_requests: usize,
    /// 为模型目录和其他控制面请求预留的并发槽位，避免被长生成请求占满。
    pub control_plane_concurrency: usize,
    pub stream_buffer_capacity: usize,
    pub graceful_shutdown_timeout: Duration,
    pub official_cloud_code_endpoint: Option<String>,
}

impl Default for HttpServerOptions {
    fn default() -> Self {
        Self {
            require_auth: false,
            require_host_auth: false,
            fallback_to_random_port_on_bind_error: false,
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrent_requests: 64,
            control_plane_concurrency: 8,
            stream_buffer_capacity: 32,
            graceful_shutdown_timeout: Duration::from_secs(15),
            official_cloud_code_endpoint: None,
        }
    }
}
