mod request;
mod response;
mod stream;

use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::response::FinishReason;
use crate::domain::{
    NeutralChatRequest, NeutralChatResponse, Provider, ProxyError, UpstreamModel, UsageInfo,
};
#[cfg(test)]
use crate::domain::{NeutralContentBlock, NeutralStreamEvent};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[cfg(test)]
use stream::OpenAIResponsesStreamDecoder;

#[derive(Default)]
pub struct OpenAIResponsesAdapter;

impl OpenAIResponsesAdapter {
    pub fn new() -> Self {
        Self
    }
}

fn normalize_finish_reason(raw: &str) -> FinishReason {
    match raw {
        "completed" | "stop" => FinishReason::Stop,
        "max_output_tokens" | "length" | "incomplete" => FinishReason::MaxTokens,
        "tool_calls" | "function_call" => FinishReason::ToolCall,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn parse_usage(value: &Value) -> Option<UsageInfo> {
    let usage = value.as_object()?;
    let token = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
    };
    let input_tokens = token(usage.get("input_tokens")).unwrap_or(0);
    let output_tokens = token(usage.get("output_tokens")).unwrap_or(0);
    let cache_read_tokens = token(
        usage
            .get("input_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| details.get("cached_tokens")),
    );
    let reasoning_tokens = token(
        usage
            .get("output_tokens_details")
            .and_then(Value::as_object)
            .and_then(|details| details.get("reasoning_tokens")),
    );

    Some(UsageInfo::from_aggregate_totals(
        input_tokens,
        output_tokens,
        token(usage.get("total_tokens")),
        cache_read_tokens,
        None,
        reasoning_tokens,
    ))
}

#[async_trait]
impl ProviderAdapter for OpenAIResponsesAdapter {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_message_tool_call_reasoning_and_usage() {
        let adapter = OpenAIResponsesAdapter::new();
        let model = UpstreamModel {
            id: "upstream".to_string(),
            provider_id: "provider".to_string(),
            upstream_model_id: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            capabilities: Default::default(),
            token_limits: Default::default(),
            compression_policy: None,
            tokenizer: None,
            parameter_overrides: Default::default(),
            enabled: true,
        };
        let response = adapter
            .parse_response(
                200,
                r#"{
                    "id":"resp_1",
                    "model":"gpt-5",
                    "status":"completed",
                    "output":[
                      {"type":"reasoning","summary":[{"type":"summary_text","text":"Plan"}]},
                      {"type":"message","content":[{"type":"output_text","text":"Done"}]},
                      {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"q\":\"x\"}"}
                    ],
                    "usage":{
                      "input_tokens":12,
                      "output_tokens":8,
                      "total_tokens":20,
                      "input_tokens_details":{"cached_tokens":5},
                      "output_tokens_details":{"reasoning_tokens":3}
                    }
                }"#,
                &model,
            )
            .unwrap();
        assert_eq!(response.id, "resp_1");
        assert_eq!(
            response.usage,
            Some(UsageInfo {
                input_tokens: 7,
                output_tokens: 5,
                cache_read_tokens: Some(5),
                cache_write_tokens: None,
                reasoning_tokens: Some(3),
                total_tokens: 20,
            })
        );
        assert!(matches!(
            response.choices[0].finish_reason,
            Some(FinishReason::ToolCall)
        ));
        assert!(response.choices[0]
            .blocks
            .iter()
            .any(|block| matches!(block, NeutralContentBlock::Thinking { .. })));
        assert!(response.choices[0].blocks.iter().any(
            |block| matches!(block, NeutralContentBlock::ToolCall { id, .. } if id == "call_1")
        ));
    }

    #[test]
    fn decodes_responses_text_and_completed_events() {
        let mut decoder = OpenAIResponsesStreamDecoder::new("gpt-5".to_string());
        let start = decoder
            .decode_data(
                r#"{"type":"response.created","response":{"id":"resp_1","model":"gpt-5"}}"#,
            )
            .unwrap();
        assert!(matches!(start[0], NeutralStreamEvent::ResponseStart { .. }));
        let delta = decoder
            .decode_data(r#"{"type":"response.output_text.delta","delta":"Hello"}"#)
            .unwrap();
        assert!(matches!(&delta[0], NeutralStreamEvent::TextDelta { text, .. } if text == "Hello"));
        let end = decoder
            .decode_data(r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"input_tokens_details":{"cached_tokens":4},"output_tokens_details":{"reasoning_tokens":3}}}}"#)
            .unwrap();
        assert!(end.iter().any(|event| matches!(
            event,
            NeutralStreamEvent::Finish {
                reason: FinishReason::Stop,
                ..
            }
        )));
        assert_eq!(
            end.last(),
            Some(&NeutralStreamEvent::ResponseEnd {
                usage: Some(UsageInfo {
                    input_tokens: 7,
                    output_tokens: 4,
                    cache_read_tokens: Some(4),
                    cache_write_tokens: None,
                    reasoning_tokens: Some(3),
                    total_tokens: 18,
                }),
            })
        );
    }
}
