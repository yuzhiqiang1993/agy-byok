mod request;
mod response;
mod stream;

use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::response::FinishReason;
use crate::domain::{
    NeutralChatRequest, NeutralChatResponse, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
pub struct OpenAIAdapter;

impl OpenAIAdapter {
    pub fn new() -> Self {
        Self
    }
}

fn normalize_finish_reason(raw_finish_reason: &str) -> FinishReason {
    match raw_finish_reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::MaxTokens,
        "tool_calls" | "function_call" => FinishReason::ToolCall,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn parse_index(value: &Value, fallback: usize) -> u32 {
    value["index"]
        .as_u64()
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(fallback as u32)
}

fn parse_usage(value: &Value) -> Option<UsageInfo> {
    let usage = value.as_object()?;
    let token = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
    };
    let prompt_tokens = token(usage.get("prompt_tokens")).unwrap_or(0);
    let completion_tokens = token(usage.get("completion_tokens")).unwrap_or(0);
    let cache_read_tokens = token(
        usage
            .get("prompt_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| details.get("cached_tokens")),
    );
    let reasoning_tokens = token(
        usage
            .get("completion_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| details.get("reasoning_tokens")),
    );

    Some(UsageInfo::from_aggregate_totals(
        prompt_tokens,
        completion_tokens,
        token(usage.get("total_tokens")),
        cache_read_tokens,
        None,
        reasoning_tokens,
    ))
}

#[async_trait]
impl ProviderAdapter for OpenAIAdapter {
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
