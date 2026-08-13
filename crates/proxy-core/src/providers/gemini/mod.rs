mod request;
mod response;
mod stream;

use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::response::FinishReason;
#[cfg(test)]
use crate::domain::{
    ErrorCategory, MessageRole, NeutralContentBlock, NeutralMessage, NeutralStreamEvent,
};
use crate::domain::{
    NeutralChatRequest, NeutralChatResponse, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[cfg(test)]
use stream::GeminiStreamDecoder;

#[derive(Default)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }

    #[cfg(test)]
    fn convert_message(message: &NeutralMessage) -> Value {
        request::convert_message(message)
    }
}

fn normalize_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::MaxTokens,
        "TOOL_CALL" => FinishReason::ToolCall,
        "SAFETY"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn parse_usage(value: &Value, current: Option<&UsageInfo>) -> Option<UsageInfo> {
    let usage = value.as_object()?;
    let token = |field: &str| {
        usage
            .get(field)
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
    };
    let cache_read_tokens = token("cachedContentTokenCount")
        .or_else(|| current.and_then(|usage| usage.cache_read_tokens));
    let reasoning_tokens =
        token("thoughtsTokenCount").or_else(|| current.and_then(|usage| usage.reasoning_tokens));
    let prompt_tokens = token("promptTokenCount")
        .or_else(|| current.map(UsageInfo::prompt_tokens))
        .unwrap_or(0);
    let completion_tokens = token("candidatesTokenCount")
        .or_else(|| current.map(|usage| usage.output_tokens))
        .unwrap_or(0)
        .saturating_add(reasoning_tokens.unwrap_or(0));
    let total_tokens = token("totalTokenCount").or_else(|| current.map(|usage| usage.total_tokens));

    Some(UsageInfo::from_aggregate_totals(
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cache_read_tokens,
        None,
        reasoning_tokens,
    ))
}

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn build_generate_endpoint(
        &self,
        provider: &Provider,
        upstream_model: &UpstreamModel,
        stream: bool,
        _request: &NeutralChatRequest,
    ) -> Result<String, ProxyError> {
        request::build_generate_endpoint(provider, upstream_model, stream)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_messages_are_encoded_as_user_turns() {
        let message = NeutralMessage {
            role: MessageRole::Tool,
            blocks: vec![NeutralContentBlock::ToolResult {
                tool_call_id: "call-lookup".to_string(),
                name: Some("lookup".to_string()),
                content: r#"{"result":"ok"}"#.to_string(),
            }],
        };

        let converted = GeminiAdapter::convert_message(&message);

        assert_eq!(converted["role"], "user");
        assert_eq!(
            converted["parts"][0]["functionResponse"]["id"],
            "call-lookup"
        );
        assert_eq!(converted["parts"][0]["functionResponse"]["name"], "lookup");
    }

    #[test]
    fn assistant_tool_call_ids_are_encoded() {
        let message = NeutralMessage {
            role: MessageRole::Assistant,
            blocks: vec![NeutralContentBlock::ToolCall {
                id: "call-lookup".to_string(),
                name: "lookup".to_string(),
                arguments_json: r#"{"query":"rust"}"#.to_string(),
            }],
        };

        let converted = GeminiAdapter::convert_message(&message);

        assert_eq!(converted["parts"][0]["functionCall"]["id"], "call-lookup");
        assert_eq!(converted["parts"][0]["functionCall"]["name"], "lookup");
    }

    #[test]
    fn thinking_signatures_are_encoded() {
        let message = NeutralMessage {
            role: MessageRole::Assistant,
            blocks: vec![NeutralContentBlock::Thinking {
                text: "summary".to_string(),
                signature: Some("signed-thought".to_string()),
            }],
        };

        let converted = GeminiAdapter::convert_message(&message);

        assert_eq!(converted["parts"][0]["text"], "summary");
        assert_eq!(converted["parts"][0]["thoughtSignature"], "signed-thought");
    }

    #[test]
    fn decoder_emits_all_candidates_and_attaches_usage_to_response_end() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());

        let mut events = decoder
            .decode_data(
                r#"{
                    "responseId":"response-1",
                    "candidates":[
                        {
                            "index":4,
                            "content":{"parts":[
                                {"text":"answer"},
                                {"thought":true,"text":"reason"},
                                {"functionCall":{"name":"lookup","args":{"query":"rust"}}}
                            ]},
                            "finishReason":"TOOL_CALL"
                        },
                        {
                            "content":{"parts":[{"text":"alternative"}]},
                            "finishReason":"MAX_TOKENS"
                        }
                    ],
                    "usageMetadata":{
                        "promptTokenCount":3,
                        "candidatesTokenCount":5,
                        "totalTokenCount":8
                    }
                }"#,
            )
            .unwrap();
        events.extend(decoder.decode_data("[DONE]").unwrap());

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: Some("response-1".to_string()),
                    model: "gemini-upstream".to_string(),
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 4,
                    text: "answer".to_string(),
                },
                NeutralStreamEvent::ThinkingDelta {
                    choice_index: 4,
                    text: "reason".to_string(),
                },
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 4,
                    tool_call_index: 2,
                    id: "call_4_2".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 4,
                    tool_call_index: 2,
                    arguments_delta: r#"{"query":"rust"}"#.to_string(),
                },
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 4,
                    tool_call_index: 2,
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 1,
                    text: "alternative".to_string(),
                },
                NeutralStreamEvent::Finish {
                    choice_index: 4,
                    reason: FinishReason::ToolCall,
                    raw_finish_reason: Some("TOOL_CALL".to_string()),
                },
                NeutralStreamEvent::Finish {
                    choice_index: 1,
                    reason: FinishReason::MaxTokens,
                    raw_finish_reason: Some("MAX_TOKENS".to_string()),
                },
                NeutralStreamEvent::ResponseEnd {
                    usage: Some(UsageInfo {
                        input_tokens: 3,
                        output_tokens: 5,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        reasoning_tokens: None,
                        total_tokens: 8,
                    }),
                },
            ]
        );
    }

    #[test]
    fn decoder_maps_blocked_prompt_to_content_filter_finish() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        let mut events = decoder
            .decode_data(
                r#"{
                    "promptFeedback":{"blockReason":"SAFETY"},
                    "usageMetadata":{"promptTokenCount":4,"totalTokenCount":4}
                }"#,
            )
            .unwrap();
        events.extend(decoder.finish().unwrap());

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: None,
                    model: "gemini-upstream".to_string(),
                },
                NeutralStreamEvent::Finish {
                    choice_index: 0,
                    reason: FinishReason::ContentFilter,
                    raw_finish_reason: Some("SAFETY".to_string()),
                },
                NeutralStreamEvent::ResponseEnd {
                    usage: Some(UsageInfo {
                        input_tokens: 4,
                        output_tokens: 0,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        reasoning_tokens: None,
                        total_tokens: 4,
                    }),
                },
            ]
        );
    }

    #[test]
    fn decoder_deduplicates_tool_parts_and_choice_finish() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        let data = r#"{
            "candidates":[{
                "index":2,
                "content":{"parts":[{
                    "functionCall":{"name":"lookup","args":{"query":"rust"}}
                }]},
                "finishReason":"STOP"
            }]
        }"#;

        let first_events = decoder.decode_data(data).unwrap();
        let repeated_events = decoder.decode_data(data).unwrap();

        assert_eq!(first_events.len(), 5);
        assert!(repeated_events.is_empty());
    }

    #[test]
    fn decoder_ends_once_for_done_or_eof() {
        let mut done_decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            done_decoder.decode_data("[DONE]").unwrap(),
            vec![NeutralStreamEvent::ResponseEnd { usage: None }]
        );
        assert!(done_decoder.decode_data("{}").unwrap().is_empty());
        assert!(done_decoder.finish().unwrap().is_empty());

        let mut eof_decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            eof_decoder.finish().unwrap(),
            vec![NeutralStreamEvent::ResponseEnd { usage: None }]
        );
        assert!(eof_decoder.finish().unwrap().is_empty());
    }

    #[test]
    fn decoder_starts_without_response_id_and_rejects_invalid_json() {
        let mut decoder = GeminiStreamDecoder::new("gemini-upstream".to_string());
        assert_eq!(
            decoder.decode_data("{}").unwrap(),
            vec![NeutralStreamEvent::ResponseStart {
                response_id: None,
                model: "gemini-upstream".to_string(),
            }]
        );

        let error = decoder.decode_data("data: {}").unwrap_err();
        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
        assert_eq!(error.status_code, 502);
    }
}
