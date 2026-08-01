use super::server::{EncodedFrameSink, ProxyServer};
use crate::antigravity::{AntigravityRequestParser, CloudCodeEnvelopeEncoder};
use crate::domain::{ErrorCategory, ProxyError};
use crate::upstream_body::read_limited_response_body;
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_stream::wrappers::ReceiverStream;

const LOCAL_TOKEN_HEADER: &str = "x-agy-byok-token";

type HttpBody = BoxBody<Bytes, Infallible>;
type HttpResponse = Response<HttpBody>;
type HttpFrame = Result<Frame<Bytes>, Infallible>;

struct HttpFrameSink {
    sender: mpsc::Sender<HttpFrame>,
}

#[async_trait]
impl EncodedFrameSink for HttpFrameSink {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        let Some(envelope) = CloudCodeEnvelopeEncoder::wrap_stream_frame(&frame)? else {
            return Ok(());
        };
        self.sender
            .send(Ok(Frame::data(Bytes::from(envelope))))
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCategory::StreamInterrupted,
                    "Downstream SSE receiver is closed",
                    499,
                )
            })
    }
}

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub require_auth: bool,
    pub require_host_auth: bool,
    pub fallback_to_random_port_on_bind_error: bool,
    pub max_body_bytes: usize,
    pub max_concurrent_requests: usize,
    pub stream_buffer_capacity: usize,
    pub graceful_shutdown_timeout: Duration,
    pub official_cloud_code_endpoint: Option<String>,
}

impl Default for HttpServerOptions {
    fn default() -> Self {
        Self {
            require_auth: true,
            require_host_auth: false,
            fallback_to_random_port_on_bind_error: false,
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrent_requests: 64,
            stream_buffer_capacity: 32,
            graceful_shutdown_timeout: Duration::from_secs(15),
            official_cloud_code_endpoint: None,
        }
    }
}

pub struct HttpServerHandle {
    local_addr: SocketAddr,
    shutdown_sender: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), ProxyError>>>,
}

impl HttpServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn shutdown(mut self) -> Result<(), ProxyError> {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        let task = self.task.take().ok_or_else(|| {
            ProxyError::new(ErrorCategory::Internal, "HTTP server task is missing", 500)
        })?;
        task.await.map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("HTTP server task failed: {error}"),
                500,
            )
        })?
    }
}

impl Drop for HttpServerHandle {
    fn drop(&mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
    }
}

pub struct LoopbackHttpServer;

impl LoopbackHttpServer {
    pub async fn start(
        proxy: Arc<ProxyServer>,
        options: HttpServerOptions,
    ) -> Result<HttpServerHandle, ProxyError> {
        if options.max_body_bytes == 0
            || options.max_concurrent_requests == 0
            || options.stream_buffer_capacity == 0
        {
            return Err(ProxyError::new(
                ErrorCategory::InvalidRequest,
                "HTTP server limits must be greater than zero",
                400,
            ));
        }

        if let Some(endpoint) = &options.official_cloud_code_endpoint {
            validate_official_endpoint(endpoint)?;
        }

        let bind_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, proxy.port()));
        let listener = match TcpListener::bind(bind_addr).await {
            Ok(listener) => listener,
            Err(primary_error)
                if options.fallback_to_random_port_on_bind_error && proxy.port() != 0 =>
            {
                let fallback_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
                tracing::warn!(
                    "Preferred proxy port {bind_addr} is unavailable; selecting a random loopback port"
                );
                TcpListener::bind(fallback_addr).await.map_err(|fallback_error| {
                    ProxyError::new(
                        ErrorCategory::ConnectionFailed,
                        format!(
                            "Failed to bind preferred loopback address {bind_addr}: {primary_error}; \
                             random loopback fallback also failed: {fallback_error}"
                        ),
                        500,
                    )
                })?
            }
            Err(error) => {
                return Err(ProxyError::new(
                    ErrorCategory::ConnectionFailed,
                    format!("Failed to bind loopback HTTP server on {bind_addr}: {error}"),
                    500,
                ));
            }
        };
        let local_addr = listener.local_addr().map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to read loopback server address: {error}"),
                500,
            )
        })?;
        if !local_addr.ip().is_loopback() {
            return Err(ProxyError::new(
                ErrorCategory::Internal,
                "HTTP server refused to bind a non-loopback address",
                500,
            ));
        }

        let semaphore = Arc::new(Semaphore::new(options.max_concurrent_requests));
        let (shutdown_sender, mut shutdown_receiver) = oneshot::channel();
        let task_options = options.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver => break,
                    accept_result = listener.accept() => {
                        let (stream, peer_addr) = accept_result.map_err(|error| {
                            ProxyError::new(
                                ErrorCategory::ConnectionFailed,
                                format!("Loopback HTTP accept failed: {error}"),
                                500,
                            )
                        })?;
                        if !peer_addr.ip().is_loopback() {
                            tracing::warn!("Rejected non-loopback peer on loopback listener");
                            continue;
                        }

                        let proxy = proxy.clone();
                        let options = task_options.clone();
                        let semaphore = semaphore.clone();
                        connections.spawn(async move {
                            let service = hyper::service::service_fn(move |request| {
                                handle_request(
                                    request,
                                    proxy.clone(),
                                    options.clone(),
                                    semaphore.clone(),
                                )
                            });
                            if let Err(error) = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection(TokioIo::new(stream), service)
                            .await
                            {
                                tracing::debug!("Loopback HTTP connection closed with error: {error}");
                            }
                        });
                    }
                    Some(join_result) = connections.join_next(), if !connections.is_empty() => {
                        if let Err(error) = join_result {
                            tracing::debug!("Loopback HTTP connection task failed: {error}");
                        }
                    }
                }
            }

            let drain_result = timeout(task_options.graceful_shutdown_timeout, async {
                while let Some(join_result) = connections.join_next().await {
                    if let Err(error) = join_result {
                        tracing::debug!("Loopback HTTP connection task failed: {error}");
                    }
                }
            })
            .await;
            if drain_result.is_err() {
                connections.abort_all();
                while connections.join_next().await.is_some() {}
            }
            Ok(())
        });

        if let Err(error) = probe_health(local_addr).await {
            let _ = shutdown_sender.send(());
            let _ = task.await;
            return Err(error);
        }

        Ok(HttpServerHandle {
            local_addr,
            shutdown_sender: Some(shutdown_sender),
            task: Some(task),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct NativeForwardOptions {
    stream: bool,
    max_response_body_bytes: usize,
    stream_buffer_capacity: usize,
}

#[derive(Debug, Clone, Copy)]
enum RouteKind {
    Health,
    Models,
    FetchModels,
    Generate,
    StreamGenerate,
    Passthrough,
}

async fn handle_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    semaphore: Arc<Semaphore>,
) -> Result<HttpResponse, Infallible> {
    tracing::info!("INCOMING PATH: {}", request.uri().path());
    let route = route_kind(request.uri().path());
    let Some(route) = route else {
        return Ok(error_response(
            StatusCode::NOT_FOUND,
            "Route not found",
            "not_found",
        ));
    };

    if request.method() == Method::OPTIONS {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            .header("Access-Control-Allow-Headers", "*")
            .header("Access-Control-Max-Age", "86400")
            .body(Full::new(Bytes::new()).boxed())
            .unwrap());
    }

    let expected_method = match route {
        RouteKind::Health | RouteKind::Models => Some(Method::GET),
        RouteKind::FetchModels | RouteKind::Generate | RouteKind::StreamGenerate => {
            Some(Method::POST)
        }
        RouteKind::Passthrough => None,
    };
    if expected_method
        .as_ref()
        .is_some_and(|expected| request.method() != expected)
    {
        return Ok(with_cors(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed for this route",
            "method_not_allowed",
        )));
    }

    if matches!(route, RouteKind::Health) {
        return Ok(with_cors(health_response()));
    }
    let route_requires_auth = match route {
        RouteKind::Models => options.require_auth,
        RouteKind::FetchModels
        | RouteKind::Generate
        | RouteKind::StreamGenerate
        | RouteKind::Passthrough => options.require_host_auth,
        RouteKind::Health => false,
    };
    if route_requires_auth && !is_authorized(&request, &proxy) {
        return Ok(with_cors(error_response(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid local proxy token",
            "authentication",
        )));
    }

    let permit = match semaphore.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(with_cors(error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Local proxy concurrency limit reached",
                "rate_limit",
            )));
        }
    };

    let response = match route {
        RouteKind::Health => unreachable!("health returned before authentication"),
        RouteKind::Models => {
            let _permit = permit;
            model_list_response(&proxy)
        }
        RouteKind::FetchModels => {
            handle_fetch_models_request(request, proxy, options, permit).await
        }
        RouteKind::Generate => {
            handle_generate_request(request, proxy, options, permit, false).await
        }
        RouteKind::StreamGenerate => {
            handle_generate_request(request, proxy, options, permit, true).await
        }
        RouteKind::Passthrough => handle_passthrough_request(request, proxy, options, permit).await,
    };
    Ok(with_cors(response))
}

fn with_cors(mut response: HttpResponse) -> HttpResponse {
    response.headers_mut().insert(
        "Access-Control-Allow-Origin",
        hyper::header::HeaderValue::from_static("*"),
    );
    response
}

fn route_kind(path: &str) -> Option<RouteKind> {
    match path {
        "/health" | "/healthz" => Some(RouteKind::Health),
        "/v1/models" | "/v1beta/models" => Some(RouteKind::Models),
        "/v1internal:fetchAvailableModels" => Some(RouteKind::FetchModels),
        "/v1internal:generateContent" => Some(RouteKind::Generate),
        "/v1internal:streamGenerateContent" => Some(RouteKind::StreamGenerate),
        _ if path.starts_with("/v1internal:") || path.starts_with("/v1internal/") => {
            Some(RouteKind::Passthrough)
        }
        _ => None,
    }
}

async fn probe_health(local_addr: SocketAddr) -> Result<(), ProxyError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to build internal health client: {error}"),
                500,
            )
        })?;
    let response = client
        .get(format!("http://{local_addr}/health"))
        .send()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::ConnectionFailed,
                format!("Internal health probe failed: {error}"),
                500,
            )
        })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(ProxyError::new(
            ErrorCategory::Internal,
            format!("Internal health probe returned {}", response.status()),
            500,
        ));
    }
    Ok(())
}

fn validate_official_endpoint(endpoint: &str) -> Result<(), ProxyError> {
    let url = reqwest::Url::parse(endpoint).map_err(|error| {
        ProxyError::new(
            ErrorCategory::InvalidRequest,
            format!("Invalid official Cloud Code endpoint: {error}"),
            400,
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            "Official Cloud Code endpoint must be an absolute HTTP(S) URL",
            400,
        ));
    }
    if url.scheme() == "http" {
        let host = url.host_str().expect("host checked above");
        let is_loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false);
        if !is_loopback {
            return Err(ProxyError::new(
                ErrorCategory::InvalidRequest,
                "Official Cloud Code endpoint must use HTTPS unless it targets Loopback",
                400,
            ));
        }
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            "Official Cloud Code endpoint cannot contain a query or fragment",
            400,
        ));
    }
    Ok(())
}

async fn handle_passthrough_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    permit: OwnedSemaphorePermit,
) -> HttpResponse {
    let Some(endpoint) = options.official_cloud_code_endpoint.as_deref() else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "Official Cloud Code forwarding is not configured",
            "native_forwarding_unavailable",
        );
    };
    let (parts, body) = match read_request(request, options.max_body_bytes).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    forward_native_request(
        parts,
        body,
        proxy,
        endpoint,
        permit,
        NativeForwardOptions {
            stream: false,
            max_response_body_bytes: options.max_body_bytes,
            stream_buffer_capacity: options.stream_buffer_capacity,
        },
    )
    .await
}

async fn handle_fetch_models_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    permit: OwnedSemaphorePermit,
) -> HttpResponse {
    let _permit = permit;
    let Some(endpoint) = options.official_cloud_code_endpoint.as_deref() else {
        return model_list_response(&proxy);
    };
    let (parts, body) = match read_request(request, options.max_body_bytes).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let response = match send_forward_request(parts, body, &proxy, endpoint).await {
        Ok(response) => response,
        Err(_) => return model_list_response(&proxy),
    };
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            "Official model catalog returned status {}, falling back to custom models",
            status
        );
        return model_list_response(&proxy);
    }
    let body = match read_limited_response_body(response, options.max_body_bytes).await {
        Ok(body) if !body.is_truncated() => body.into_bytes(),
        Ok(_) | Err(_) => return model_list_response(&proxy),
    };
    let upstream_models: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(models) => models,
        Err(_) => return model_list_response(&proxy),
    };
    let models = proxy.handle_model_list(upstream_models);
    full_response(StatusCode::OK, "application/json", models.to_string())
}

async fn forward_native_request(
    parts: hyper::http::request::Parts,
    body: Bytes,
    proxy: Arc<ProxyServer>,
    endpoint: &str,
    permit: OwnedSemaphorePermit,
    options: NativeForwardOptions,
) -> HttpResponse {
    let response = match send_forward_request(parts, body, &proxy, endpoint).await {
        Ok(response) => response,
        Err(response) => return response,
    };
    let status = response.status();
    tracing::info!("NATIVE FORWARD STATUS: {}", status);
    let headers = response.headers().clone();
    if !options.stream {
        let _permit = permit;
        return match read_limited_response_body(response, options.max_response_body_bytes).await {
            Ok(body) if !body.is_truncated() => {
                bytes_response(status, &headers, Bytes::from(body.into_bytes()))
            }
            Ok(_) => error_response(
                StatusCode::BAD_GATEWAY,
                "Official response body exceeds the buffered response limit",
                "native_forwarding_failed",
            ),
            Err(error) => error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read official response: {error}"),
                "native_forwarding_failed",
            ),
        };
    }

    let (sender, receiver) = mpsc::channel(options.stream_buffer_capacity);
    tokio::spawn(async move {
        let _permit = permit;
        let mut upstream = response.bytes_stream();
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(bytes) => {
                    if sender
                        .send(Ok::<_, Infallible>(Frame::data(bytes)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    tracing::debug!("Official Cloud Code stream ended with error: {error}");
                    break;
                }
            }
        }
    });

    let mut builder = Response::builder().status(status);
    for (name, value) in &headers {
        if !is_hop_by_hop_header(name.as_str()) && !is_cors_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(BodyExt::boxed(StreamBody::new(ReceiverStream::new(
            receiver,
        ))))
        .expect("valid forwarded streaming response")
}

async fn send_forward_request(
    parts: hyper::http::request::Parts,
    body: Bytes,
    proxy: &ProxyServer,
    endpoint: &str,
) -> Result<reqwest::Response, HttpResponse> {
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(parts.uri.path());
    let target = format!("{}{}", endpoint.trim_end_matches('/'), path_and_query);
    let mut request = proxy
        .http_client()
        .request(parts.method, target)
        .header(ACCEPT_ENCODING, "identity")
        .body(body);
    for (name, value) in &parts.headers {
        if !is_hop_by_hop_header(name.as_str())
            && !name.as_str().eq_ignore_ascii_case(LOCAL_TOKEN_HEADER)
            && *name != ACCEPT_ENCODING
        {
            request = request.header(name, value);
        }
    }
    request.send().await.map_err(|error| {
        error_response(
            StatusCode::BAD_GATEWAY,
            &format!("Failed to forward request to official Cloud Code: {error}"),
            "native_forwarding_failed",
        )
    })
}

fn is_cors_header(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("access-control-")
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn bytes_response(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: Bytes,
) -> HttpResponse {
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if !is_hop_by_hop_header(name.as_str()) && !is_cors_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Full::new(body).boxed())
        .expect("valid forwarded HTTP response")
}

async fn handle_generate_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    permit: OwnedSemaphorePermit,
    stream: bool,
) -> HttpResponse {
    let (parts, body) = match read_request(request, options.max_body_bytes).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let body_text = match std::str::from_utf8(&body) {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Request body must be valid UTF-8 JSON",
                "invalid_request",
            )
        }
    };
    let model_id = match AntigravityRequestParser::extract_model_id(body_text) {
        Ok(model_id) => model_id,
        Err(error) => return proxy_error_response(&error),
    };

    if !proxy.is_custom_model_id(&model_id) {
        let Some(endpoint) = options.official_cloud_code_endpoint.as_deref() else {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "Native model forwarding is not configured",
                "native_forwarding_unavailable",
            );
        };
        let request_shape = AntigravityRequestParser::parse(body_text).ok();
        let started = Instant::now();
        let response = forward_native_request(
            parts,
            body,
            proxy.clone(),
            endpoint,
            permit,
            NativeForwardOptions {
                stream,
                max_response_body_bytes: options.max_body_bytes,
                stream_buffer_capacity: options.stream_buffer_capacity,
            },
        )
        .await;
        proxy.record_official_generation(
            &model_id,
            stream,
            request_shape
                .as_ref()
                .map(|request| request.messages.len())
                .unwrap_or(0),
            request_shape
                .as_ref()
                .map(|request| request.tools.len())
                .unwrap_or(0),
            response.status().as_u16(),
            started.elapsed().as_millis() as u64,
        );
        return response;
    }

    let mut neutral_request = match AntigravityRequestParser::parse(body_text) {
        Ok(request) => request,
        Err(error) => return proxy_error_response(&error),
    };
    neutral_request.stream = stream;

    if !stream {
        let _permit = permit;
        return match proxy.handle_chat_request(&neutral_request).await {
            Ok(body) => match CloudCodeEnvelopeEncoder::wrap_response(&body) {
                Ok(envelope) => full_response(StatusCode::OK, "application/json", envelope),
                Err(error) => proxy_error_response(&error),
            },
            Err(error) => proxy_error_response(&error),
        };
    }

    let (sender, receiver) = mpsc::channel(options.stream_buffer_capacity);
    tokio::spawn(async move {
        let _permit = permit;
        let error_sender = sender.clone();
        let mut frame_sink = HttpFrameSink { sender };
        let stream_result = proxy
            .handle_chat_stream_to(&neutral_request, &mut frame_sink)
            .await;

        if let Err(error) = stream_result {
            let payload = json!({
                "error": {
                    "code": error.status_code,
                    "category": format!("{:?}", error.category),
                    "message": error.message
                }
            });
            let error_frame = format!("data: {}\n\n", payload);
            if let Ok(Some(envelope)) = CloudCodeEnvelopeEncoder::wrap_stream_frame(&error_frame) {
                let _ = error_sender
                    .send(Ok(Frame::data(Bytes::from(envelope))))
                    .await;
            }
        }
    });

    let body = BodyExt::boxed(StreamBody::new(ReceiverStream::new(receiver)));
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .expect("valid streaming HTTP response")
}

async fn read_request(
    request: Request<Incoming>,
    max_body_bytes: usize,
) -> Result<(hyper::http::request::Parts, Bytes), HttpResponse> {
    if let Some(content_length) = request.headers().get(CONTENT_LENGTH) {
        let content_length = content_length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "Invalid Content-Length header",
                    "invalid_request",
                )
            })?;
        if content_length > max_body_bytes {
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds the configured limit",
                "payload_too_large",
            ));
        }
    }

    let (parts, body) = request.into_parts();
    let collected = Limited::new(body, max_body_bytes)
        .collect()
        .await
        .map_err(|_| {
            error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds the configured limit or could not be read",
                "payload_too_large",
            )
        })?;
    Ok((parts, collected.to_bytes()))
}

fn is_authorized(request: &Request<Incoming>, proxy: &ProxyServer) -> bool {
    let local_token = request
        .headers()
        .get(LOCAL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    if local_token.is_some_and(|token| proxy.auth_manager().validate_token(token)) {
        return true;
    }

    let authorization = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    proxy.auth_manager().validate_header(authorization)
}

fn health_response() -> HttpResponse {
    full_response(
        StatusCode::OK,
        "application/json",
        json!({
            "status": "ok",
            "product": "agy-byok",
            "version": env!("CARGO_PKG_VERSION"),
            "capabilities": {
                "models": true,
                "generate": true,
                "stream": true
            }
        })
        .to_string(),
    )
}

fn model_list_response(proxy: &ProxyServer) -> HttpResponse {
    let models = proxy.handle_model_list(json!({ "models": [] }));
    full_response(StatusCode::OK, "application/json", models.to_string())
}

fn proxy_error_response(error: &ProxyError) -> HttpResponse {
    let status =
        StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    error_response(status, &error.message, &format!("{:?}", error.category))
}

fn error_response(status: StatusCode, message: &str, category: &str) -> HttpResponse {
    full_response(
        status,
        "application/json",
        json!({
            "error": {
                "code": status.as_u16(),
                "category": category,
                "message": message
            }
        })
        .to_string(),
    )
}

fn full_response(
    status: StatusCode,
    content_type: &'static str,
    body: impl Into<Bytes>,
) -> HttpResponse {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(Full::new(body.into()).boxed())
        .expect("valid HTTP response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_frame_sink_waits_for_bounded_channel_capacity() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut sink = HttpFrameSink { sender };
        let frame = "data: {\"candidates\":[]}\n\n".to_string();

        sink.send(frame.clone()).await.unwrap();

        let mut second_send = tokio::spawn(async move { sink.send(frame).await });
        assert!(
            timeout(Duration::from_millis(25), &mut second_send)
                .await
                .is_err(),
            "the second frame must wait while the bounded channel is full"
        );

        receiver.recv().await.unwrap().unwrap();
        timeout(Duration::from_secs(1), second_send)
            .await
            .expect("the second send should resume after capacity is available")
            .expect("the second send task should complete")
            .expect("the second frame should be sent successfully");
        receiver.recv().await.unwrap().unwrap();
    }

    #[test]
    fn forwarded_responses_drop_upstream_cors_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "access-control-allow-origin",
            reqwest::header::HeaderValue::from_static("*"),
        );
        headers.insert(
            "access-control-allow-credentials",
            reqwest::header::HeaderValue::from_static("true"),
        );
        headers.insert(
            CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let response = bytes_response(StatusCode::OK, &headers, Bytes::new());

        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
        assert!(response
            .headers()
            .get("access-control-allow-credentials")
            .is_none());
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
