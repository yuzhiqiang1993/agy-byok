use super::request::read_request;
use super::responses::{
    bytes_response, error_response, fetch_models_fallback_response,
    fetch_models_fallback_response_with_summary, full_response, is_cors_header,
    is_hop_by_hop_header, with_response_summary,
};
use super::types::{HttpResponse, HttpServerOptions, LOCAL_TOKEN_HEADER};
use crate::antigravity::AntigravityModelDescriptor;
use crate::domain::{ErrorCategory, ProxyError};
use crate::proxy::activity::ActivityErrorCategory;
use crate::proxy::server::ProxyServer;
use crate::upstream_body::read_limited_response_body;
use bytes::Bytes;
use futures::StreamExt;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;
use hyper::header::ACCEPT_ENCODING;
use hyper::{Request, StatusCode};
use reqwest::Url;
use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, OwnedSemaphorePermit};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Clone, Copy)]
pub(super) struct NativeForwardOptions {
    pub(super) stream: bool,
    pub(super) max_response_body_bytes: usize,
    pub(super) stream_buffer_capacity: usize,
}

pub(super) async fn handle_passthrough_request(
    request: Request<hyper::body::Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    permit: OwnedSemaphorePermit,
) -> HttpResponse {
    let Some(endpoint) = options.official_cloud_code_endpoint.as_deref() else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "Official Cloud Code forwarding is not configured",
            ActivityErrorCategory::NativeForwardingUnavailable,
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

pub(super) async fn handle_fetch_models_request(
    request: Request<hyper::body::Incoming>,
    proxy: Arc<ProxyServer>,
    options: HttpServerOptions,
    permit: OwnedSemaphorePermit,
) -> HttpResponse {
    let _permit = permit;
    let Some(endpoint) = options.official_cloud_code_endpoint.as_deref() else {
        return fetch_models_fallback_response(&proxy);
    };
    let (parts, body) = match read_request(request, options.max_body_bytes).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let forward_fut = send_forward_request(&parts, body, &proxy, endpoint);
    let response = match tokio::time::timeout(std::time::Duration::from_secs(4), forward_fut).await
    {
        Ok(Ok(response)) => response,
        _ => {
            return fetch_models_fallback_response_with_summary(
                &proxy,
                "source=custom; official=unavailable",
            )
        }
    };
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            "Official model catalog returned status {}, falling back to custom models",
            status
        );
        return fetch_models_fallback_response_with_summary(
            &proxy,
            format!("source=custom; official_status={}", status.as_u16()),
        );
    }
    let body = match read_limited_response_body(response, options.max_body_bytes).await {
        Ok(body) if !body.is_truncated() => body.into_bytes(),
        Ok(_) => {
            return fetch_models_fallback_response_with_summary(
                &proxy,
                "source=custom; official_body=too_large",
            )
        }
        Err(_) => {
            return fetch_models_fallback_response_with_summary(
                &proxy,
                "source=custom; official_body=unreadable",
            )
        }
    };
    let mut upstream_models: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(models) => models,
        Err(_) => {
            return fetch_models_fallback_response_with_summary(
                &proxy,
                "source=custom; official_body=invalid_json",
            )
        }
    };

    // Cache the RAW, valid JSON catalog before any modifications are applied.
    // This allows the desktop app UI to fetch the completely unfiltered list,
    // including disabled official models, even though they will be removed below.
    if let Ok(raw_json_str) = serde_json::to_string(&upstream_models) {
        proxy.config_store().set_raw_official_catalog(raw_json_str);
    }

    if let Some(obj) = upstream_models.as_object_mut() {
        obj.remove("error");
    }
    let proxy_target = get_proxy_target_host(&parts, &proxy);
    let rewritten_models_str = proxy.prepare_model_catalog_response(upstream_models, &proxy_target);
    let catalog_count = serde_json::from_str::<serde_json::Value>(&rewritten_models_str)
        .map(|models| AntigravityModelDescriptor::model_count(&models))
        .unwrap_or(0);
    with_response_summary(
        full_response(StatusCode::OK, "application/json", rewritten_models_str),
        format!("catalog_models={catalog_count}; source=official"),
    )
}

pub(super) async fn forward_native_request(
    parts: hyper::http::request::Parts,
    body: Bytes,
    proxy: Arc<ProxyServer>,
    endpoint: &str,
    permit: OwnedSemaphorePermit,
    options: NativeForwardOptions,
) -> HttpResponse {
    let forward_fut = send_forward_request(&parts, body, &proxy, endpoint);
    let response = match tokio::time::timeout(std::time::Duration::from_secs(120), forward_fut).await
    {
        Ok(Ok(response)) => response,
        Ok(Err(response)) => return response,
        Err(_) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "Failed to forward request to official Cloud Code within timeout",
                ActivityErrorCategory::NativeForwardingFailed,
            );
        }
    };
    let status = response.status();
    tracing::info!("NATIVE FORWARD STATUS: {}", status);
    let headers = response.headers().clone();
    if !options.stream {
        let _permit = permit;
        return match read_limited_response_body(response, options.max_response_body_bytes).await {
            Ok(body) if !body.is_truncated() => {
                let bytes = rewrite_official_urls(Bytes::from(body.into_bytes()), &parts, &proxy);
                with_response_summary(
                    bytes_response(status, &headers, bytes),
                    "source=official; forwarded=true",
                )
            }
            Ok(_) => error_response(
                StatusCode::BAD_GATEWAY,
                "Official response body exceeds the buffered response limit",
                ActivityErrorCategory::NativeForwardingFailed,
            ),
            Err(error) => error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read official response: {error}"),
                ActivityErrorCategory::NativeForwardingFailed,
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

    let mut builder = hyper::Response::builder().status(status);
    for (name, value) in &headers {
        if !is_hop_by_hop_header(name.as_str()) && !is_cors_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }
    let response = builder
        .body(BodyExt::boxed(StreamBody::new(ReceiverStream::new(
            receiver,
        ))))
        .expect("valid forwarded streaming response");
    with_response_summary(response, "source=official; forwarded=true; stream=true")
}

async fn send_forward_request(
    parts: &hyper::http::request::Parts,
    body: Bytes,
    proxy: &ProxyServer,
    endpoint: &str,
) -> Result<reqwest::Response, HttpResponse> {
    let path_and_query = super::lifecycle::LoopbackHttpServer::normalize_path_and_query(&parts.uri);
    let target = format!("{}{}", endpoint.trim_end_matches('/'), path_and_query);
    let mut request = proxy
        .http_client()
        .request(parts.method.clone(), target)
        .header(ACCEPT_ENCODING, "identity")
        .body(body);
    for (name, value) in &parts.headers {
        if !is_hop_by_hop_header(name.as_str())
            && !name.as_str().eq_ignore_ascii_case(LOCAL_TOKEN_HEADER)
            && *name != hyper::header::HOST
            && *name != ACCEPT_ENCODING
        {
            request = request.header(name, value);
        }
    }
    request.send().await.map_err(|error| {
        error_response(
            StatusCode::BAD_GATEWAY,
            &format!("Failed to forward request to official Cloud Code: {error}"),
            ActivityErrorCategory::NativeForwardingFailed,
        )
    })
}

fn get_proxy_target_host(parts: &hyper::http::request::Parts, proxy: &ProxyServer) -> String {
    if let Some(host_val) = parts.headers.get(hyper::header::HOST) {
        if let Ok(host_str) = host_val.to_str() {
            let host_str = host_str.trim();
            if !host_str.is_empty() && !host_str.ends_with(".googleapis.com") {
                if host_str.starts_with("http://") || host_str.starts_with("https://") {
                    return host_str.to_string();
                }
                return format!("http://{host_str}");
            }
        }
    }
    format!("http://127.0.0.1:{}", proxy.port())
}

fn rewrite_official_urls(
    bytes: Bytes,
    parts: &hyper::http::request::Parts,
    proxy: &ProxyServer,
) -> Bytes {
    let proxy_target = get_proxy_target_host(parts, proxy);
    let slice = bytes.as_ref();
    if !slice
        .windows(b"googleapis.com".len())
        .any(|w| w == b"googleapis.com")
    {
        return bytes;
    }

    if let Ok(body_str) = std::str::from_utf8(&bytes) {
        let modified = rewrite_official_urls_str(body_str, &proxy_target);
        Bytes::from(modified.into_bytes())
    } else {
        bytes
    }
}

pub(crate) fn rewrite_official_urls_str(text: &str, proxy_target: &str) -> String {
    text.replace("https://daily-cloudcode-pa.googleapis.com", proxy_target)
        .replace("https://cloudcode-pa.googleapis.com", proxy_target)
        .replace(
            "https://daily-cloudaicompanion-pa.googleapis.com",
            proxy_target,
        )
        .replace("https://cloudaicompanion-pa.googleapis.com", proxy_target)
        .replace(
            "https://daily-cloudcode-pa.sandbox.googleapis.com",
            proxy_target,
        )
        .replace("https://cloudcode-pa.sandbox.googleapis.com", proxy_target)
        .replace("https://generativelanguage.googleapis.com", proxy_target)
}

pub(super) fn validate_official_endpoint(endpoint: &str) -> Result<(), ProxyError> {
    let url = Url::parse(endpoint).map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AppConfig;
    use crate::storage::ConfigStore;
    use hyper::header::HOST;

    #[test]
    fn get_proxy_target_host_ignores_upstream_google_hosts() {
        let store = ConfigStore::in_memory(AppConfig::default());
        let proxy = ProxyServer::new(store, 12345);

        let (mut parts, _) = hyper::Request::builder().body(()).unwrap().into_parts();
        parts
            .headers
            .insert(HOST, "daily-cloudcode-pa.googleapis.com".parse().unwrap());

        let target = get_proxy_target_host(&parts, &proxy);
        assert_eq!(target, "http://127.0.0.1:12345");

        parts
            .headers
            .insert(HOST, "127.0.0.1:12345".parse().unwrap());
        let target2 = get_proxy_target_host(&parts, &proxy);
        assert_eq!(target2, "http://127.0.0.1:12345");
    }
}
