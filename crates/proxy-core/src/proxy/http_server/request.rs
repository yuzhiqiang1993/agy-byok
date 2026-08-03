use super::responses::error_response;
use super::types::HttpResponse;
use bytes::Bytes;
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use hyper::header::CONTENT_LENGTH;
use hyper::Request;
use hyper::StatusCode;

pub(super) async fn read_request(
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
