use super::forwarding::{forward_native_request, NativeForwardOptions};
use super::request::read_request;
use super::responses::{error_response, full_response, proxy_error_response};
use super::streaming::HttpFrameSink;
use super::types::{HttpResponse, HttpServerOptions};
use crate::antigravity::{AntigravityRequestParser, CloudCodeEnvelopeEncoder};
use crate::domain::{ErrorCategory, ProxyError};
use crate::proxy::activity::ActivityErrorCategory;
use crate::proxy::server::ProxyServer;
use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use serde_json::json;

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit};
use tokio_stream::wrappers::ReceiverStream;

pub(super) async fn handle_generate_request(
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
                ActivityErrorCategory::InvalidRequest,
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
                ActivityErrorCategory::NativeForwardingUnavailable,
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

    let (startup_sender, startup_receiver) = oneshot::channel();
    let (sender, receiver) = mpsc::channel(options.stream_buffer_capacity);
    tokio::spawn(async move {
        let _permit = permit;
        let error_sender = sender.clone();
        let mut frame_sink = HttpFrameSink {
            sender,
            startup_sender: Some(startup_sender),
        };
        let stream_result = proxy
            .handle_chat_stream_to(&neutral_request, &mut frame_sink)
            .await;

        if let Err(error) = stream_result {
            if frame_sink.reject_start(&error) {
                return;
            }

            // HTTP 200 已发送后无法再改状态码，使用顶层错误帧让 IDE 立即终止当前流。
            let payload = json!({
                "error": {
                    "code": error.status_code,
                    "category": error.category.as_str(),
                    "message": error.message
                }
            });
            let error_frame = format!("data: {}\n\n", payload);
            let _ = error_sender
                .send(Ok(Frame::data(Bytes::from(error_frame))))
                .await;
        }
    });

    match startup_receiver.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return proxy_error_response(&error),
        Err(_) => {
            return proxy_error_response(&ProxyError::new(
                ErrorCategory::Internal,
                "Streaming task ended before reporting its startup status",
                500,
            ))
        }
    }

    let body = BodyExt::boxed(StreamBody::new(ReceiverStream::new(receiver)));
    Response::builder()
        .status(StatusCode::OK)
        .header(
            hyper::header::CONTENT_TYPE,
            "text/event-stream; charset=utf-8",
        )
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .expect("valid streaming HTTP response")
}
