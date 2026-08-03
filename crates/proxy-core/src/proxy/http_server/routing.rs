use super::forwarding::{handle_fetch_models_request, handle_passthrough_request};
use super::generation::handle_generate_request;
use super::lifecycle::LoopbackHttpServer;
use super::responses::{error_response, health_response, model_list_response, with_cors};
use super::types::{HttpResponse, HttpServerOptions, LOCAL_TOKEN_HEADER};
use crate::proxy::server::ProxyServer;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::Arc;
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
    semaphore: Arc<Semaphore>,
) -> Result<HttpResponse, Infallible> {
    tracing::info!(
        method = %request.method(),
        path = %request.uri().path(),
        "Incoming proxy request"
    );
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
            .body(http_body_util::Full::new(bytes::Bytes::new()).boxed())
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

fn route_kind(path: &str) -> Option<RouteKind> {
    let norm = LoopbackHttpServer::normalize_path(path);
    let p = norm.as_str();
    if p == "/health" || p == "/healthz" {
        return Some(RouteKind::Health);
    }
    if p == "/v1/models" || p == "/v1beta/models" {
        return Some(RouteKind::Models);
    }
    if p.contains("fetchAvailableModels") || p.contains("GetAvailableModels") {
        return Some(RouteKind::FetchModels);
    }
    if p.contains("streamGenerateContent") {
        return Some(RouteKind::StreamGenerate);
    }
    if p.contains("generateContent") {
        return Some(RouteKind::Generate);
    }
    Some(RouteKind::Passthrough)
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
