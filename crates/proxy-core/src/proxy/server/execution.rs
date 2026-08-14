use super::{token_guard, ProxyServer};
use crate::antigravity::AntigravityStreamEncoder;
use crate::domain::{
    ErrorCategory, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralStreamEvent, ProviderProtocol, ProxyError, UsageInfo,
};
use crate::providers::{get_adapter, is_image_generation_request, ProviderAdapter};
use crate::proxy::streaming::{NeutralEventSink, StreamPipe};
use crate::routing::ResolvedRoute;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use async_trait::async_trait;
use reqwest::Response;
use std::sync::Arc;
use std::time::Duration;

#[async_trait]
pub(crate) trait EncodedFrameSink: Send {
    /// 上游已接受流式请求，此后下游可以安全发送 HTTP 200 响应头。
    fn stream_started(&mut self) {}

    async fn send(&mut self, frame: String) -> Result<(), ProxyError>;
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

fn should_downgrade_image_generation_stream(
    protocol: &ProviderProtocol,
    is_image_generation: bool,
) -> bool {
    is_image_generation && matches!(protocol, ProviderProtocol::OpenaiChatCompletions)
}

impl ProxyServer {
    pub(super) async fn execute_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<(NeutralChatResponse, Option<UsageInfo>), ProxyError> {
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
        let mut neutral_response = adapter.parse_response(status, &body, &route.upstream_model)?;
        if neutral_response.model.trim().is_empty() {
            neutral_response
                .model
                .clone_from(&route.upstream_model.upstream_model_id);
        }
        let usage = neutral_response.usage.clone();
        Ok((neutral_response, usage))
    }

    pub(super) async fn execute_stream_route(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
        emitted_frame: &mut bool,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        // OpenAI images 端点不支持流式，生图请求统一降级为非流式处理后回传流式事件。
        if should_downgrade_image_generation_stream(
            &route.provider.protocol,
            is_image_generation_request(&route.upstream_model, request),
        ) {
            return self
                .execute_image_generation_stream(route, request, frame_sink, emitted_frame)
                .await;
        }

        let (adapter, response) = self.send_upstream(route, request).await?;
        let status = response.status().as_u16();
        if status >= 400 {
            // 错误响应体同样可能停滞，沿用流空闲超时避免启动握手永久等待。
            let error_body_timeout_ms = route.provider.stream_idle_timeout_ms;
            let buffered = tokio::time::timeout(
                Duration::from_millis(error_body_timeout_ms),
                read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES),
            )
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCategory::Timeout,
                    format!("Upstream error body timeout after {error_body_timeout_ms} ms"),
                    504,
                )
            })?
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

        frame_sink.stream_started();

        let mut provider_decoder = adapter.create_stream_decoder(&route.upstream_model);
        let mut usage = None;
        {
            let mut event_sink = AntigravityEventSink {
                encoder: AntigravityStreamEncoder::new()
                    .with_model_version(&route.upstream_model.upstream_model_id),
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

    /// 图片生成降级路径：走非流式 images 请求，再把完整结果按流式事件回传。
    async fn execute_image_generation_stream(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
        emitted_frame: &mut bool,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        let (response, usage) = self.execute_route(route, request).await?;
        frame_sink.stream_started();

        let mut encoder = AntigravityStreamEncoder::new()
            .with_model_version(&route.upstream_model.upstream_model_id);

        for choice in &response.choices {
            for block in &choice.blocks {
                let event = match block {
                    NeutralContentBlock::Text(text) => NeutralStreamEvent::TextDelta {
                        choice_index: choice.index,
                        text: text.clone(),
                    },
                    NeutralContentBlock::InlineData {
                        mime_type,
                        data_base64,
                    } => {
                        let clean_base64 = data_base64.replace(['\r', '\n', ' '], "");
                        NeutralStreamEvent::InlineData {
                            choice_index: choice.index,
                            mime_type: mime_type.clone(),
                            data_base64: clean_base64,
                        }
                    }
                    _ => continue,
                };
                for frame in encoder.encode_event(&event)? {
                    *emitted_frame = true;
                    frame_sink.send(frame).await?;
                }
            }
            if let Some(reason) = choice.finish_reason {
                for frame in encoder.encode_event(&NeutralStreamEvent::Finish {
                    choice_index: choice.index,
                    reason,
                    raw_finish_reason: choice.raw_finish_reason.clone(),
                })? {
                    *emitted_frame = true;
                    frame_sink.send(frame).await?;
                }
            }
        }

        for frame in encoder.encode_event(&NeutralStreamEvent::ResponseEnd {
            usage: usage.clone(),
        })? {
            *emitted_frame = true;
            frame_sink.send(frame).await?;
        }
        Ok(usage)
    }

    pub(super) async fn send_upstream(
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
            request,
        )?;
        let is_image_request = route
            .upstream_model
            .capabilities
            .roles
            .contains(&crate::domain::ModelRole::ImageGeneration)
            || route
                .upstream_model
                .capabilities
                .supports_output(crate::domain::ModelModality::Image)
            || request
                .output_modalities
                .contains(&crate::domain::ModelModality::Image);

        let request_timeout_ms = if is_image_request {
            route.provider.request_timeout_ms.max(120_000)
        } else {
            route.provider.request_timeout_ms
        };
        let connect_timeout_ms = route.provider.connect_timeout_ms;
        let client = self.provider_http_client(connect_timeout_ms)?;

        let mut request_builder = client.post(generate_endpoint).json(&payload);
        for (name, value) in headers {
            request_builder = request_builder.header(name, value);
        }

        let response_result = if request.stream {
            // 流式请求只限制响应头等待时间，响应体由逐块空闲超时保护。
            tokio::time::timeout(
                Duration::from_millis(request_timeout_ms),
                request_builder.send(),
            )
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCategory::Timeout,
                    format!("Upstream response header timeout after {request_timeout_ms} ms"),
                    504,
                )
            })?
        } else {
            request_builder
                .timeout(Duration::from_millis(request_timeout_ms))
                .send()
                .await
        };
        let response = response_result.map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::should_downgrade_image_generation_stream;
    use crate::domain::ProviderProtocol;

    #[test]
    fn only_openai_chat_image_requests_use_non_streaming_fallback() {
        assert!(should_downgrade_image_generation_stream(
            &ProviderProtocol::OpenaiChatCompletions,
            true,
        ));
        assert!(!should_downgrade_image_generation_stream(
            &ProviderProtocol::GeminiGenerateContent,
            true,
        ));
        assert!(!should_downgrade_image_generation_stream(
            &ProviderProtocol::AnthropicMessages,
            true,
        ));
        assert!(!should_downgrade_image_generation_stream(
            &ProviderProtocol::OpenaiChatCompletions,
            false,
        ));
    }
}
