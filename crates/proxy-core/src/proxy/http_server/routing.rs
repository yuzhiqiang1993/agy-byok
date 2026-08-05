use super::forwarding::{handle_fetch_models_request, handle_passthrough_request};
use super::generation::handle_generate_request;
use super::lifecycle::LoopbackHttpServer;
use super::responses::{
    error_response, health_response, model_list_response, with_cors, with_response_summary,
};
use super::types::{
    HttpActivityMetadata, HttpResponse, HttpServerOptions, INTERNAL_PROBE_HEADER,
    LOCAL_TOKEN_HEADER,
};
use crate::proxy::server::ProxyServer;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::CONTENT_LENGTH;
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy)]
enum RouteKind {
    Health,
    Models,
    FetchModels,
    Generate,
    StreamGenerate,
    Passthrough,
}

pub(super) async fn handle_request(
    request: Request<Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    generation_semaphore: Arc<Semaphore>,
    control_plane_semaphore: Arc<Semaphore>,
) -> Result<HttpResponse, Infallible> {
    let started = Instant::now();
    let method = request.method().as_str().to_string();
    let path = LoopbackHttpServer::normalize_path(request.uri().path());
    let request_body_bytes = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let route = route_kind(request.uri().path());
    let internal_probe = request
        .headers()
        .get(INTERNAL_PROBE_HEADER)
        .is_some_and(|value| value == "1");

    tracing::info!(
        method = %method,
        path = %path,
        "Incoming proxy request"
    );

    let response = if request.method() == Method::OPTIONS {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            .header("Access-Control-Allow-Headers", "*")
            .header("Access-Control-Max-Age", "86400")
            .header(CONTENT_LENGTH, "0")
            .body(http_body_util::Full::new(Bytes::new()).boxed())
            .expect("valid CORS preflight response");
        with_response_summary(response, "cors_preflight")
    } else {
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
            with_cors(error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "Method not allowed for this route",
                "method_not_allowed",
            ))
        } else if matches!(route, RouteKind::Health) {
            with_cors(health_response())
        } else {
            let route_requires_auth = match route {
                RouteKind::Models => options.require_auth,
                RouteKind::FetchModels
                | RouteKind::Generate
                | RouteKind::StreamGenerate
                | RouteKind::Passthrough => options.require_host_auth,
                RouteKind::Health => false,
            };

            if route_requires_auth && !is_authorized(&request, &proxy) {
                with_cors(error_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing or invalid local proxy token",
                    "authentication",
                ))
            } else {
                let semaphore = if matches!(route, RouteKind::Generate | RouteKind::StreamGenerate)
                {
                    generation_semaphore
                } else {
                    control_plane_semaphore
                };
                let permit = match semaphore.try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        let response = error_response(
                            StatusCode::TOO_MANY_REQUESTS,
                            "Local proxy concurrency limit reached",
                            "rate_limit",
                        );
                        let response = with_cors(response);
                        if should_record_http_activity(route, &method, internal_probe) {
                            record_http_activity(
                                &proxy,
                                route,
                                &method,
                                &path,
                                request_body_bytes,
                                started,
                                &response,
                            );
                        }
                        return Ok(response);
                    }
                };

                let response = match route {
                    RouteKind::Health => unreachable!("health returned before authentication"),
                    RouteKind::Models => {
                        let _permit = permit;
                        model_list_response(&proxy)
                    }
                    RouteKind::FetchModels => {
                        handle_fetch_models_request(request, proxy.clone(), options, permit).await
                    }
                    RouteKind::Generate => {
                        handle_generate_request(request, proxy.clone(), options, permit, false)
                            .await
                    }
                    RouteKind::StreamGenerate => {
                        handle_generate_request(request, proxy.clone(), options, permit, true).await
                    }
                    RouteKind::Passthrough => {
                        handle_passthrough_request(request, proxy.clone(), options, permit).await
                    }
                };
                with_cors(response)
            }
        }
    };

    if should_record_http_activity(route, &method, internal_probe) {
        record_http_activity(
            &proxy,
            route,
            &method,
            &path,
            request_body_bytes,
            started,
            &response,
        );
    }
    Ok(response)
}

fn route_kind(path: &str) -> RouteKind {
    let norm = LoopbackHttpServer::normalize_path(path);
    let p = norm.as_str();
    if p == "/health" || p == "/healthz" {
        return RouteKind::Health;
    }
    if p == "/v1/models" || p == "/v1beta/models" {
        return RouteKind::Models;
    }
    if p.contains("fetchAvailableModels") || p.contains("GetAvailableModels") {
        return RouteKind::FetchModels;
    }
    if p.contains("streamGenerateContent") {
        return RouteKind::StreamGenerate;
    }
    if p.contains("generateContent") {
        return RouteKind::Generate;
    }
    RouteKind::Passthrough
}

fn operation_name(route: RouteKind, method: &str) -> &'static str {
    if method.eq_ignore_ascii_case("OPTIONS") {
        return "cors_preflight";
    }
    match route {
        RouteKind::Health => "health_check",
        RouteKind::Models => "list_models",
        RouteKind::FetchModels => "fetch_available_models",
        RouteKind::Generate => "generate",
        RouteKind::StreamGenerate => "stream_generate",
        RouteKind::Passthrough => "passthrough",
    }
}

fn should_record_http_activity(route: RouteKind, method: &str, internal_probe: bool) -> bool {
    if internal_probe && matches!(route, RouteKind::Health) {
        return false;
    }
    !(method.eq_ignore_ascii_case("POST")
        && matches!(route, RouteKind::Generate | RouteKind::StreamGenerate))
}

fn record_http_activity(
    proxy: &ProxyServer,
    route: RouteKind,
    method: &str,
    path: &str,
    request_body_bytes: Option<u64>,
    started: Instant,
    response: &HttpResponse,
) {
    let metadata = response
        .extensions()
        .get::<HttpActivityMetadata>()
        .cloned()
        .unwrap_or_default();
    let response_body_bytes = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    proxy.record_http_activity(
        operation_name(route, method),
        method,
        path,
        request_body_bytes,
        response.status().as_u16(),
        started.elapsed().as_millis() as u64,
        response_body_bytes,
        metadata.response_summary.as_deref(),
        metadata.error_category.as_deref(),
        metadata.error_detail.as_deref(),
    );
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
        .get(hyper::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    proxy.auth_manager().validate_header(authorization)
}
