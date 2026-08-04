use super::types::{HttpActivityMetadata, HttpResponse};
use crate::domain::ProxyError;
use crate::proxy::server::ProxyServer;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::CONTENT_TYPE;
use hyper::{Response, StatusCode};
use serde_json::json;

pub(super) fn with_cors(mut response: HttpResponse) -> HttpResponse {
    response.headers_mut().insert(
        "Access-Control-Allow-Origin",
        hyper::header::HeaderValue::from_static("*"),
    );
    response
}

pub(super) fn is_cors_header(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("access-control-")
}

pub(super) fn is_hop_by_hop_header(name: &str) -> bool {
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
            | "content-encoding"
    )
}

pub(super) fn bytes_response(
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
        .header(hyper::header::CONTENT_LENGTH, body.len().to_string())
        .body(Full::new(body).boxed())
        .expect("valid forwarded HTTP response")
}

pub(super) fn health_response() -> HttpResponse {
    with_response_summary(
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
        ),
        "status=ok",
    )
}

pub(super) fn model_list_response(proxy: &ProxyServer) -> HttpResponse {
    let models = proxy.handle_model_list(json!({ "models": [] }));
    let count = models
        .get("models")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    with_response_summary(
        full_response(StatusCode::OK, "application/json", models.to_string()),
        format!("models={count}"),
    )
}

pub(super) fn fetch_models_fallback_response(proxy: &ProxyServer) -> HttpResponse {
    fetch_models_fallback_response_with_summary(proxy, "source=custom")
}

pub(super) fn fetch_models_fallback_response_with_summary(
    proxy: &ProxyServer,
    source_summary: impl Into<String>,
) -> HttpResponse {
    let models = proxy.handle_model_list(json!({ "models": {} }));
    let count = models
        .get("models")
        .and_then(serde_json::Value::as_object)
        .map_or(0, serde_json::Map::len);
    with_response_summary(
        full_response(StatusCode::OK, "application/json", models.to_string()),
        format!("catalog_models={count}; {}", source_summary.into()),
    )
}

pub(super) fn with_response_summary(
    mut response: HttpResponse,
    summary: impl Into<String>,
) -> HttpResponse {
    response.extensions_mut().insert(HttpActivityMetadata {
        response_summary: Some(summary.into()),
        ..HttpActivityMetadata::default()
    });
    response
}

pub(super) fn proxy_error_response(error: &ProxyError) -> HttpResponse {
    let status =
        StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    error_response(status, &error.message, &format!("{:?}", error.category))
}

pub(super) fn error_response(status: StatusCode, message: &str, category: &str) -> HttpResponse {
    let mut response = full_response(
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
    );
    response.extensions_mut().insert(HttpActivityMetadata {
        response_summary: None,
        error_category: Some(category.to_string()),
        error_detail: Some(message.to_string()),
    });
    response
}

pub(super) fn full_response(
    status: StatusCode,
    content_type: &'static str,
    body: impl Into<Bytes>,
) -> HttpResponse {
    let body = body.into();
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .header(hyper::header::CONTENT_LENGTH, body.len().to_string())
        .body(Full::new(body).boxed())
        .expect("valid HTTP response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use hyper::header::CONTENT_TYPE;
    use hyper::StatusCode;

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
