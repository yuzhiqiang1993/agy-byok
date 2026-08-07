use super::{token_guard, ProxyServer};
use crate::antigravity::{AntigravityResponseEncoder, AntigravityStreamEncoder};
use crate::domain::{ErrorCategory, NeutralChatRequest, NeutralStreamEvent, ProxyError, UsageInfo};
use crate::providers::{get_adapter, ProviderAdapter};
use crate::proxy::streaming::{NeutralEventSink, StreamPipe};
use crate::routing::ResolvedRoute;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use async_trait::async_trait;
use reqwest::Response;
use std::sync::Arc;
use std::time::Duration;

pub(super) const DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS: u64 = 5_000;
pub(super) const DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS: u64 = 60_000;

#[async_trait]
pub(crate) trait EncodedFrameSink: Send {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError>;
}

pub(super) struct CallbackFrameSink<F> {
    pub(super) callback: F,
}

#[async_trait]
impl<F> EncodedFrameSink for CallbackFrameSink<F>
where
    F: FnMut(String) -> Result<(), ProxyError> + Send,
{
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        (self.callback)(frame)
    }
}

pub(super) struct StringFrameSink<'a> {
    pub(super) buffer: &'a mut String,
}

#[async_trait]
impl EncodedFrameSink for StringFrameSink<'_> {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        self.buffer.push_str(&frame);
        Ok(())
    }
}

struct AntigravityEventSink<'a> {
    encoder: AntigravityStreamEncoder,
    frame_sink: &'a mut dyn EncodedFrameSink,
    emitted_frame: &'a mut bool,
    usage: &'a mut Option<UsageInfo>,
}

#[async_trait]
impl NeutralEventSink for AntigravityEventSink<'_> {
    async fn send(&mut self, event: NeutralStreamEvent) -> Result<(), ProxyError> {
        if let NeutralStreamEvent::ResponseEnd { usage } = &event {
            *self.usage = usage.clone();
        }
        for frame in self.encoder.encode_event(&event)? {
            *self.emitted_frame = true;
            self.frame_sink.send(frame).await?;
        }
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), ProxyError> {
        for frame in self.encoder.abort() {
            *self.emitted_frame = true;
            self.frame_sink.send(frame).await?;
        }
        Ok(())
    }
}

impl ProxyServer {
    pub(super) async fn execute_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(String, Option<UsageInfo>), ProxyError> {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        let buffered =
            read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES)
                .await
                .map_err(|error| {
                    ProxyError::new(
                        ErrorCategory::Internal,
                        format!("Failed to read upstream response body: {error}"),
                        500,
                    )
                })?;
        let truncated = buffered.is_truncated();
        let body = buffered.into_text();
        if truncated && status < 400 {
            return Err(upstream_body_too_large_error());
        }
        let body = if truncated {
            format!("{body}\n[upstream error body exceeded the buffered response limit]")
        } else {
            body
        };
        let neutral_response = adapter.parse_response(status, &body, &route.upstream_model)?;
        let usage = neutral_response.usage.clone();
        Ok((
            AntigravityResponseEncoder::encode_response(&neutral_response),
            usage,
        ))
    }

    pub(super) async fn execute_stream_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
        emitted_frame: &mut bool,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        if status >= 400 {
            let buffered =
                read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES)
                    .await
                    .map_err(|error| {
                        ProxyError::new(
                            ErrorCategory::Internal,
                            format!("Failed to read upstream error body: {error}"),
                            500,
                        )
                    })?;
            let truncated = buffered.is_truncated();
            let body = buffered.into_text();
            let body = if truncated {
                format!("{body}\n[upstream error body exceeded the buffered response limit]")
            } else {
                body
            };
            return match adapter.parse_response(status, &body, &route.upstream_model) {
                Err(error) => Err(error),
                Ok(_) => Err(ProxyError::new(
                    ErrorCategory::UpstreamServerError,
                    format!("Unexpected successful parse for upstream status {status}"),
                    502,
                )),
            };
        }

        let mut provider_decoder = adapter.create_stream_decoder(&route.upstream_model);
        let mut usage = None;
        {
            let mut event_sink = AntigravityEventSink {
                encoder: AntigravityStreamEncoder::new(),
                frame_sink,
                emitted_frame,
                usage: &mut usage,
            };
            StreamPipe::process_stream_to(
                response,
                route.provider.stream_idle_timeout_ms,
                provider_decoder.as_mut(),
                &mut event_sink,
            )
            .await?;
        }
        Ok(usage)
    }

    async fn send_upstream(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(Arc<dyn ProviderAdapter>, Response), ProxyError> {
        let adapter = get_adapter(&route.provider.protocol);
        let payload = adapter.build_request_payload(route, request)?;
        token_guard::validate_request(route, request).await?;
        let headers = adapter.build_headers(&route.provider)?;
        let generate_endpoint = adapter.build_generate_endpoint(
            &route.provider,
            &route.upstream_model,
            request.stream,
        )?;
        let request_timeout_ms =
            effective_provider_request_timeout_ms(route.provider.request_timeout_ms);
        let connect_timeout_ms = effective_provider_connect_timeout_ms(
            route.provider.connect_timeout_ms,
            request_timeout_ms,
        );
        let client = self.provider_http_client(connect_timeout_ms)?;

        let mut request_builder = client.post(generate_endpoint).json(&payload);
        for (name, value) in headers {
            request_builder = request_builder.header(name, value);
        }

        let response = request_builder
            .timeout(Duration::from_millis(request_timeout_ms))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProxyError::new(
                        ErrorCategory::Timeout,
                        format!("Upstream timeout: {error}"),
                        504,
                    )
                } else {
                    ProxyError::new(
                        ErrorCategory::ConnectionFailed,
                        format!("Failed to connect to upstream: {error}"),
                        502,
                    )
                }
            })?;

        Ok((adapter, response))
    }
}

pub(super) fn effective_provider_request_timeout_ms(configured_timeout_ms: u64) -> u64 {
    match configured_timeout_ms {
        0 => DEFAULT_PROVIDER_REQUEST_TIMEOUT_MS,
        configured => configured,
    }
}

pub(super) fn effective_provider_connect_timeout_ms(
    configured_timeout_ms: u64,
    request_timeout_ms: u64,
) -> u64 {
    match configured_timeout_ms {
        0 => DEFAULT_PROVIDER_CONNECT_TIMEOUT_MS,
        configured => configured,
    }
    .min(request_timeout_ms)
}

fn upstream_body_too_large_error() -> ProxyError {
    ProxyError::new(
        ErrorCategory::UpstreamServerError,
        format!(
            "Upstream response body exceeds {} bytes",
            DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES
        ),
        502,
    )
}
