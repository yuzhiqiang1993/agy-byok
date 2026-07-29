use super::server::ProxyServer;
use crate::antigravity::{AntigravityRequestParser, CloudCodeEnvelopeEncoder};
use crate::domain::{ErrorCategory, ProxyError};
use bytes::Bytes;
use futures::StreamExt;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_stream::wrappers::ReceiverStream;

const LOCAL_TOKEN_HEADER: &str = "x-agy-byok-token";

type HttpBody = BoxBody<Bytes, Infallible>;
type HttpResponse = Response<HttpBody>;

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub require_auth: bool,
    pub require_host_auth: bool,
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
        let listener = TcpListener::bind(bind_addr).await.map_err(|error| {
            ProxyError::new(
                ErrorCategory::ConnectionFailed,
                format!("Failed to bind loopback HTTP server on {bind_addr}: {error}"),
                500,
            )
        })?;
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
enum RouteKind {
    Health,
    Models,
    FetchModels,
    Generate,
    StreamGenerate,
}

async fn handle_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    semaphore: Arc<Semaphore>,
) -> Result<HttpResponse, Infallible> {
    let route = route_kind(request.uri().path());
    let Some(route) = route else {
        return Ok(error_response(
            StatusCode::NOT_FOUND,
            "Route not found",
            "not_found",
        ));
    };

    let expected_method = match route {
        RouteKind::Health | RouteKind::Models => Method::GET,
        RouteKind::FetchModels | RouteKind::Generate | RouteKind::StreamGenerate => Method::POST,
    };
    if request.method() != expected_method {
        return Ok(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed for this route",
            "method_not_allowed",
        ));
    }

    if matches!(route, RouteKind::Health) {
        return Ok(health_response());
    }
    let route_requires_auth = match route {
        RouteKind::Models => options.require_auth,
        RouteKind::FetchModels | RouteKind::Generate | RouteKind::StreamGenerate => {
            options.require_host_auth
        }
        RouteKind::Health => false,
    };
    if route_requires_auth && !is_authorized(&request, &proxy) {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid local proxy token",
            "authentication",
        ));
    }

    let permit = match semaphore.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Local proxy concurrency limit reached",
                "rate_limit",
            ));
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
    };
    Ok(response)
}

fn route_kind(path: &str) -> Option<RouteKind> {
    match path {
        "/health" | "/healthz" => Some(RouteKind::Health),
        "/v1/models" | "/v1beta/models" => Some(RouteKind::Models),
        "/v1internal:fetchAvailableModels" => Some(RouteKind::FetchModels),
        "/v1internal:generateContent" => Some(RouteKind::Generate),
        "/v1internal:streamGenerateContent" => Some(RouteKind::StreamGenerate),
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
        Err(response) => return response,
    };
    let status = response.status();
    let headers = response.headers().clone();
    if response
        .content_length()
        .is_some_and(|length| length > options.max_body_bytes as u64)
    {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "Official model catalog exceeds the configured limit",
            "upstream_response_too_large",
        );
    }
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(error) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read official model catalog: {error}"),
                "native_forwarding_failed",
            )
        }
    };
    if body.len() > options.max_body_bytes {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "Official model catalog exceeds the configured limit",
            "upstream_response_too_large",
        );
    }
    if !status.is_success() {
        return bytes_response(status, &headers, body);
    }
    let upstream_models: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(models) => models,
        Err(error) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Official model catalog is not valid JSON: {error}"),
                "invalid_upstream_response",
            )
        }
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
    stream: bool,
    stream_buffer_capacity: usize,
) -> HttpResponse {
    let response = match send_forward_request(parts, body, &proxy, endpoint).await {
        Ok(response) => response,
        Err(response) => return response,
    };
    let status = response.status();
    let headers = response.headers().clone();
    if !stream {
        let _permit = permit;
        return match response.bytes().await {
            Ok(body) => bytes_response(status, &headers, body),
            Err(error) => error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read official response: {error}"),
                "native_forwarding_failed",
            ),
        };
    }

    let (sender, receiver) = mpsc::channel(stream_buffer_capacity);
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
        if !is_hop_by_hop_header(name.as_str()) {
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
    let mut request = proxy.http_client().request(parts.method, target).body(body);
    for (name, value) in &parts.headers {
        if !is_hop_by_hop_header(name.as_str())
            && !name.as_str().eq_ignore_ascii_case(LOCAL_TOKEN_HEADER)
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
        if !is_hop_by_hop_header(name.as_str()) {
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
        return forward_native_request(
            parts,
            body,
            proxy,
            endpoint,
            permit,
            stream,
            options.stream_buffer_capacity,
        )
        .await;
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
        let stream_result = proxy
            .handle_chat_stream(&neutral_request, |frame| {
                let Some(envelope) = CloudCodeEnvelopeEncoder::wrap_stream_frame(&frame)? else {
                    return Ok(());
                };
                sender
                    .try_send(Ok(Frame::data(Bytes::from(envelope))))
                    .map_err(|error| {
                        ProxyError::new(
                            ErrorCategory::StreamInterrupted,
                            format!("Downstream SSE receiver is unavailable: {error}"),
                            499,
                        )
                    })
            })
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
                let _ = sender.try_send(Ok(Frame::data(Bytes::from(envelope))));
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
