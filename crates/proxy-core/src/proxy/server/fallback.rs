use super::activity_recorder::ActivityOutcome;
use super::execution::{EncodedFrameSink, StringFrameSink};
use super::ProxyServer;
use crate::antigravity::AntigravityResponseEncoder;
use crate::domain::{ErrorCategory, NeutralChatRequest, ProxyError};
use crate::routing::RouteTable;
use std::time::Instant;

impl ProxyServer {

    /// 处理单个中立聊天请求，包含 Adapter 转译、网络发送与备用路由降级
    pub(crate) async fn handle_chat_request(
        &self,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        if request.stream {
            let mut encoded_stream = String::new();
            let mut frame_sink = StringFrameSink {
                buffer: &mut encoded_stream,
            };
            self.handle_chat_stream_to(request, &mut frame_sink).await?;
            return Ok(encoded_stream);
        }

        let start_time = Instant::now();
        let config = self.config_store.get_config();

        let route = match RouteTable::resolve(&config, request) {
            Ok(route) => route,
            Err(error) => {
                self.record_activity(
                    None,
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &error,
                    ),
                );
                return Err(error);
            }
        };

        match self.execute_route(&route, request).await {
            Ok((response, usage)) => {
                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::success(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        usage.as_ref(),
                    ),
                );
                Ok(AntigravityResponseEncoder::encode_response(&response))
            }
            Err(primary_error) => {
                if primary_error.is_retryable_for_fallback() {
                    match RouteTable::resolve_fallback(&config, &route, request) {
                        Ok(Some(fallback_route)) => {
                            tracing::info!(
                                "Primary route {} failed with {:?}, attempting fallback to {}",
                                route.virtual_model.id,
                                primary_error.category,
                                fallback_route.virtual_model.id
                            );

                            match self.execute_route(&fallback_route, request).await {
                                Ok((fallback_response, usage)) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::success(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            usage.as_ref(),
                                        ),
                                    );
                                    return Ok(AntigravityResponseEncoder::encode_response(
                                        &fallback_response,
                                    ));
                                }
                                Err(fallback_error) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::failure(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            &fallback_error,
                                        ),
                                    );
                                    return Err(fallback_error);
                                }
                            }
                        }
                        Err(fallback_error) => {
                            self.record_activity(
                                Some(&route),
                                request,
                                ActivityOutcome::failure(
                                    start_time.elapsed().as_millis() as u64,
                                    true,
                                    &fallback_error,
                                ),
                            );
                            return Err(fallback_error);
                        }
                        Ok(None) => {}
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &primary_error,
                    ),
                );
                Err(primary_error)
            }
        }
    }

    pub(crate) async fn handle_chat_stream_to(
        &self,
        request: &NeutralChatRequest,
        frame_sink: &mut dyn EncodedFrameSink,
    ) -> Result<(), ProxyError> {
        if !request.stream {
            return Err(ProxyError::new(
                ErrorCategory::InvalidRequest,
                "Streaming handler requires request.stream = true",
                400,
            ));
        }

        let start_time = Instant::now();
        let config = self.config_store.get_config();
        let route = match RouteTable::resolve(&config, request) {
            Ok(route) => route,
            Err(error) => {
                self.record_activity(
                    None,
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &error,
                    ),
                );
                return Err(error);
            }
        };

        let mut emitted_frame = false;
        match self
            .execute_stream_route(&route, request, frame_sink, &mut emitted_frame)
            .await
        {
            Ok(usage) => {
                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::success(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        usage.as_ref(),
                    ),
                );
                Ok(())
            }
            Err(primary_error) => {
                if !emitted_frame && primary_error.is_retryable_for_fallback() {
                    match RouteTable::resolve_fallback(&config, &route, request) {
                        Ok(Some(fallback_route)) => {
                            tracing::info!(
                                "Primary stream route {} failed with {:?}, attempting fallback to {}",
                                route.virtual_model.id,
                                primary_error.category,
                                fallback_route.virtual_model.id
                            );

                            match self
                                .execute_stream_route(
                                    &fallback_route,
                                    request,
                                    frame_sink,
                                    &mut emitted_frame,
                                )
                                .await
                            {
                                Ok(usage) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::success(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            usage.as_ref(),
                                        ),
                                    );
                                    return Ok(());
                                }
                                Err(fallback_error) => {
                                    self.record_activity(
                                        Some(&fallback_route),
                                        request,
                                        ActivityOutcome::failure(
                                            start_time.elapsed().as_millis() as u64,
                                            true,
                                            &fallback_error,
                                        ),
                                    );
                                    return Err(fallback_error);
                                }
                            }
                        }
                        Err(fallback_error) => {
                            self.record_activity(
                                Some(&route),
                                request,
                                ActivityOutcome::failure(
                                    start_time.elapsed().as_millis() as u64,
                                    true,
                                    &fallback_error,
                                ),
                            );
                            return Err(fallback_error);
                        }
                        Ok(None) => {}
                    }
                }

                self.record_activity(
                    Some(&route),
                    request,
                    ActivityOutcome::failure(
                        start_time.elapsed().as_millis() as u64,
                        false,
                        &primary_error,
                    ),
                );
                Err(primary_error)
            }
        }
    }
}
