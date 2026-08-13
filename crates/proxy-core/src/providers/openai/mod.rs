mod request;
mod response;
mod stream;

use super::is_image_generation_request;
use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::response::FinishReason;
use crate::domain::{
    ErrorCategory, NeutralChatRequest, NeutralChatResponse, Provider, ProxyError, UpstreamModel,
    UsageInfo,
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

/// 由 chat/responses 生成端点推导同源的 images 端点。
/// 例如 `https://api.openai.com/v1/chat/completions` -> `https://api.openai.com/v1/images/generations`。
fn images_generation_endpoint(generate_endpoint: &str) -> Result<String, ProxyError> {
    let mut url = reqwest::Url::parse(generate_endpoint).map_err(|error| {
        ProxyError::new(
            ErrorCategory::InvalidRequest,
            format!("Invalid OpenAI generate endpoint: {error}"),
            400,
        )
    })?;
    let path = url.path().trim_end_matches('/');
    let base = path
        .strip_suffix("/chat/completions")
        .or_else(|| path.strip_suffix("/responses"))
        .unwrap_or("/v1");
    url.set_path(&format!("{base}/images/generations"));
    Ok(url.to_string())
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
    fn build_generate_endpoint(
        &self,
        provider: &Provider,
        upstream_model: &UpstreamModel,
        _stream: bool,
        request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        if is_image_generation_request(upstream_model, request) {
            return images_generation_endpoint(&provider.generate_endpoint);
        }
        Ok(provider
            .generate_endpoint
            .replace("{model}", &upstream_model.upstream_model_id))
    }

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
