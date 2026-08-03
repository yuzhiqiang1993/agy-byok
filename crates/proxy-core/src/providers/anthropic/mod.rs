mod request;
mod response;
mod stream;

use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::response::FinishReason;
use crate::domain::{NeutralChatRequest, NeutralChatResponse, Provider, ProxyError, UpstreamModel};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

const DEFAULT_MAX_TOKENS: u32 = 4096;

#[derive(Default)]
pub struct AnthropicAdapter;

impl AnthropicAdapter {
    pub fn new() -> Self {
        Self
    }
}

fn normalize_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" => FinishReason::MaxTokens,
        "tool_use" => FinishReason::ToolCall,
        _ => FinishReason::Other,
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<Value, ProxyError> {
        request::build_request_payload(route, request)
    }

    fn build_headers(&self, provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
        request::build_headers(provider)
    }

    fn parse_response(
        &self,
        status: u16,
        body: &str,
        upstream_model: &UpstreamModel,
    ) -> Result<NeutralChatResponse, ProxyError> {
        response::parse_response(status, body, upstream_model)
    }

    fn create_stream_decoder(
        &self,
        upstream_model: &UpstreamModel,
    ) -> Box<dyn ProviderStreamDecoder> {
        stream::create_stream_decoder(upstream_model)
    }
}
