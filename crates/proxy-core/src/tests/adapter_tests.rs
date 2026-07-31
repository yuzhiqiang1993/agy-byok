#[cfg(test)]
mod tests {
    use crate::antigravity::AntigravityRequestParser;
    use crate::domain::*;
    use crate::providers::*;
    use crate::routing::{ResolvedRoute, RouteTable};
    use crate::storage::AppConfig;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};

    fn reasoning_capability(
        entries: impl IntoIterator<Item = (ReasoningLevel, ReasoningMapping)>,
    ) -> ReasoningCapability {
        ReasoningCapability {
            levels: entries.into_iter().collect::<BTreeMap<_, _>>(),
        }
    }

    fn create_provider(protocol: ProviderProtocol) -> Provider {
        Provider {
            id: "p-1".to_string(),
            name: "Test Provider".to_string(),
            protocol,
            models_endpoint: "http://localhost/models".to_string(),
            generate_endpoint: "http://localhost/chat/completions".to_string(),
            api_key: "sk-test".to_string(),
            headers: HashMap::new(),
            default_parameters: ParameterOverrides::default(),
            connect_timeout_ms: 5000,
            request_timeout_ms: 10000,
            stream_idle_timeout_ms: 10000,
            enabled: true,
        }
    }

    fn create_upstream_model(reasoning: ReasoningCapability) -> UpstreamModel {
        UpstreamModel {
            id: "um-1".to_string(),
            provider_id: "p-1".to_string(),
            upstream_model_id: "test-model".to_string(),
            display_name: "Test Model".to_string(),
            capabilities: ModelCapabilities {
                vision: true,
                tools: true,
                reasoning,
            },
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        }
    }

    fn create_virtual_model(default_reasoning_level: Option<ReasoningLevel>) -> VirtualModel {
        VirtualModel {
            id: "vm-1".to_string(),
            host_model_id: None,
            upstream_model_id: "um-1".to_string(),
            display_name: "Virtual Model 1".to_string(),
            default_reasoning_level,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        }
    }

    fn create_dummy_route(protocol: ProviderProtocol, mapping: ReasoningMapping) -> ResolvedRoute {
        ResolvedRoute {
            virtual_model: create_virtual_model(Some(ReasoningLevel::High)),
            upstream_model: create_upstream_model(reasoning_capability([(
                ReasoningLevel::High,
                mapping,
            )])),
            provider: create_provider(protocol),
            final_parameters: ParameterOverrides {
                temperature: Some(0.7),
                max_tokens: Some(2048),
                top_p: None,
                top_k: None,
                extra_body: None,
            },
            final_reasoning_level: Some(ReasoningLevel::High),
        }
    }

    fn basic_request() -> NeutralChatRequest {
        NeutralChatRequest {
            virtual_model_id: "vm-1".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Hello".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        }
    }

    #[test]
    fn reasoning_level_uses_snake_case_serde_names() {
        assert_eq!(
            serde_json::to_string(&ReasoningLevel::XHigh).unwrap(),
            "\"x_high\""
        );
        assert_eq!(
            serde_json::from_str::<ReasoningLevel>("\"x_high\"").unwrap(),
            ReasoningLevel::XHigh
        );
    }

    #[test]
    fn parameter_merge_uses_child_priority() {
        let parent = ParameterOverrides {
            temperature: Some(0.5),
            max_tokens: Some(1000),
            top_p: None,
            top_k: None,
            extra_body: None,
        };
        let child = ParameterOverrides {
            temperature: Some(0.9),
            max_tokens: None,
            top_p: Some(0.95),
            top_k: None,
            extra_body: None,
        };

        let merged = parent.merge_with(&child);
        assert_eq!(merged.temperature, Some(0.9));
        assert_eq!(merged.max_tokens, Some(1000));
        assert_eq!(merged.top_p, Some(0.95));
    }

    #[test]
    fn extra_body_blacklist_removes_controlled_fields() {
        let mut extra = HashMap::new();
        extra.insert("model".to_string(), json!("hacked-model"));
        extra.insert("messages".to_string(), json!([]));
        extra.insert("custom_option".to_string(), json!("safe_value"));

        RouteTable::sanitize_extra_body(&mut extra);

        assert!(!extra.contains_key("model"));
        assert!(!extra.contains_key("messages"));
        assert_eq!(extra.get("custom_option").unwrap(), "safe_value");
    }

    #[test]
    fn request_reasoning_level_overrides_virtual_model_default() {
        let config = AppConfig {
            proxy_port: 51234,
            providers: vec![create_provider(ProviderProtocol::Openai)],
            upstream_models: vec![create_upstream_model(reasoning_capability([
                (
                    ReasoningLevel::Low,
                    ReasoningMapping::Effort("low".to_string()),
                ),
                (
                    ReasoningLevel::High,
                    ReasoningMapping::Effort("high".to_string()),
                ),
            ]))],
            virtual_models: vec![create_virtual_model(Some(ReasoningLevel::High))],
        };
        let mut request = basic_request();
        request.reasoning_level = Some(ReasoningLevel::Low);

        let route = RouteTable::resolve(&config, &request).unwrap();

        assert_eq!(route.final_reasoning_level, Some(ReasoningLevel::Low));
    }

    #[test]
    fn unsupported_reasoning_level_fails_during_routing() {
        let config = AppConfig {
            proxy_port: 51234,
            providers: vec![create_provider(ProviderProtocol::Openai)],
            upstream_models: vec![create_upstream_model(reasoning_capability([(
                ReasoningLevel::High,
                ReasoningMapping::Effort("high".to_string()),
            )]))],
            virtual_models: vec![create_virtual_model(None)],
        };
        let mut request = basic_request();
        request.reasoning_level = Some(ReasoningLevel::Max);

        let error = RouteTable::resolve(&config, &request).unwrap_err();

        assert_eq!(error.category, ErrorCategory::UnsupportedFeature);
    }

    #[test]
    fn openai_reasoning_payload_overrides_extra_body() {
        let adapter = OpenAIAdapter::new();
        let mut route = create_dummy_route(
            ProviderProtocol::Openai,
            ReasoningMapping::Effort("high".to_string()),
        );
        route.final_parameters.extra_body = Some(HashMap::from([(
            "reasoning_effort".to_string(),
            json!("low"),
        )]));

        let payload = adapter
            .build_request_payload(&route, &basic_request())
            .unwrap();

        assert_eq!(payload["reasoning_effort"], "high");
    }

    #[test]
    fn openai_preserves_tool_call_and_result_pairing() {
        let adapter = OpenAIAdapter::new();
        let route = create_dummy_route(
            ProviderProtocol::Openai,
            ReasoningMapping::Effort("high".to_string()),
        );
        let request = AntigravityRequestParser::parse(
            &json!({
                "model": "vm-1",
                "contents": [
                    {
                        "role": "model",
                        "parts": [{
                            "functionCall": {
                                "id": "call-1",
                                "name": "view_file",
                                "args": { "path": "src/main.rs" }
                            }
                        }]
                    },
                    {
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "id": "call-1",
                                "name": "view_file",
                                "response": { "content": "file contents" }
                            }
                        }]
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let payload = adapter.build_request_payload(&route, &request).unwrap();

        assert_eq!(payload["messages"][0]["role"], "assistant");
        assert_eq!(payload["messages"][0]["tool_calls"][0]["id"], "call-1");
        assert_eq!(payload["messages"][1]["role"], "tool");
        assert_eq!(payload["messages"][1]["tool_call_id"], "call-1");
        assert_eq!(
            payload["messages"][1]["content"],
            r#"{"content":"file contents"}"#
        );
    }

    #[test]
    fn openai_normalizes_gemini_tool_schema_types() {
        let adapter = OpenAIAdapter::new();
        let route = create_dummy_route(
            ProviderProtocol::Openai,
            ReasoningMapping::Effort("high".to_string()),
        );
        let mut request = basic_request();
        request.tools = vec![NeutralTool {
            function: NeutralToolFunction {
                name: "ask_permission".to_string(),
                description: Some("Ask for permission".to_string()),
                parameters_schema: json!({
                    "type": "OBJECT",
                    "properties": {
                        "message": { "type": "STRING" },
                        "choices": {
                            "type": "ARRAY",
                            "items": {
                                "type": "OBJECT",
                                "properties": {
                                    "label": { "type": ["STRING", "NULL"] }
                                }
                            }
                        }
                    }
                }),
            },
        }];

        let payload = adapter.build_request_payload(&route, &request).unwrap();
        let parameters = &payload["tools"][0]["function"]["parameters"];

        assert_eq!(parameters["type"], "object");
        assert_eq!(parameters["properties"]["message"]["type"], "string");
        assert_eq!(parameters["properties"]["choices"]["type"], "array");
        assert_eq!(
            parameters["properties"]["choices"]["items"]["type"],
            "object"
        );
        assert_eq!(
            parameters["properties"]["choices"]["items"]["properties"]["label"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn openai_keeps_thinking_separate_from_visible_content() {
        let adapter = OpenAIAdapter::new();
        let route = create_dummy_route(
            ProviderProtocol::Openai,
            ReasoningMapping::Effort("high".to_string()),
        );
        let mut request = basic_request();
        request.messages = vec![NeutralMessage {
            role: MessageRole::Assistant,
            blocks: vec![
                NeutralContentBlock::Thinking {
                    text: "private thought".to_string(),
                    signature: None,
                },
                NeutralContentBlock::Text("visible answer".to_string()),
            ],
        }];

        let payload = adapter.build_request_payload(&route, &request).unwrap();

        assert_eq!(
            payload["messages"][0]["reasoning_content"],
            "private thought"
        );
        assert_eq!(
            payload["messages"][0]["content"][0]["text"],
            "visible answer"
        );
        assert!(!payload.to_string().contains("<thinking>"));
    }

    #[test]
    fn anthropic_reasoning_payload_uses_budget_tokens() {
        let adapter = AnthropicAdapter::new();
        let mut route = create_dummy_route(
            ProviderProtocol::Anthropic,
            ReasoningMapping::BudgetTokens(4096),
        );
        route.final_parameters.extra_body = Some(HashMap::from([(
            "thinking".to_string(),
            json!({ "type": "disabled" }),
        )]));

        let payload = adapter
            .build_request_payload(&route, &basic_request())
            .unwrap();

        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["thinking"]["budget_tokens"], 4096);
    }

    #[test]
    fn gemini_reasoning_payload_preserves_generation_config() {
        let adapter = GeminiAdapter::new();
        let mut route = create_dummy_route(
            ProviderProtocol::Gemini,
            ReasoningMapping::NativeLevel("HIGH".to_string()),
        );
        route.final_parameters.extra_body = Some(HashMap::from([(
            "generationConfig".to_string(),
            json!({
                "temperature": 0.2,
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 0,
                    "thinkingLevel": "LOW"
                }
            }),
        )]));

        let payload = adapter
            .build_request_payload(&route, &basic_request())
            .unwrap();

        assert_eq!(payload["generationConfig"]["temperature"], 0.2);
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "HIGH"
        );
        assert!(payload["generationConfig"]["thinkingConfig"]
            .get("thinkingBudget")
            .is_none());
    }

    #[test]
    fn adapter_rejects_protocol_incompatible_reasoning_mapping() {
        let adapter = OpenAIAdapter::new();
        let route = create_dummy_route(
            ProviderProtocol::Openai,
            ReasoningMapping::BudgetTokens(2048),
        );

        let error = adapter
            .build_request_payload(&route, &basic_request())
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::UnsupportedFeature);
    }

    #[test]
    fn openai_response_preserves_all_choices_and_finish_reasons() {
        let adapter = OpenAIAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let body = json!({
            "id": "chat-1",
            "model": "gpt-test",
            "choices": [
                {
                    "index": 2,
                    "message": { "content": "first", "reasoning_content": "thought" },
                    "finish_reason": "stop"
                },
                {
                    "index": 7,
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call-7",
                            "function": { "name": "lookup", "arguments": "{\"id\":7}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }
            ]
        })
        .to_string();

        let response = adapter.parse_response(200, &body, &upstream).unwrap();

        assert_eq!(response.choices.len(), 2);
        assert_eq!(response.choices[0].index, 2);
        assert_eq!(response.choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            response.choices[0].raw_finish_reason.as_deref(),
            Some("stop")
        );
        assert_eq!(response.choices[1].index, 7);
        assert_eq!(
            response.choices[1].finish_reason,
            Some(FinishReason::ToolCall)
        );
    }

    #[test]
    fn gemini_response_preserves_all_candidates_and_unique_tool_ids() {
        let adapter = GeminiAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let body = json!({
            "candidates": [
                {
                    "index": 4,
                    "content": { "parts": [
                        { "thought": true, "text": "thinking" },
                        { "functionCall": { "name": "lookup", "args": { "id": 1 } } }
                    ] },
                    "finishReason": "STOP"
                },
                {
                    "content": { "parts": [
                        { "functionCall": { "name": "lookup", "args": { "id": 2 } } }
                    ] },
                    "finishReason": "MAX_TOKENS"
                },
                {
                    "content": { "parts": [] },
                    "finishReason": "IMAGE_SAFETY"
                }
            ]
        })
        .to_string();

        let response = adapter.parse_response(200, &body, &upstream).unwrap();

        assert_eq!(response.choices.len(), 3);
        assert_eq!(response.choices[0].index, 4);
        assert_eq!(response.choices[1].index, 1);
        assert_eq!(
            response.choices[1].finish_reason,
            Some(FinishReason::MaxTokens)
        );
        assert_eq!(
            response.choices[2].finish_reason,
            Some(FinishReason::ContentFilter)
        );
        let first_id = match &response.choices[0].blocks[1] {
            NeutralContentBlock::ToolCall { id, .. } => id,
            block => panic!("expected tool call, got {block:?}"),
        };
        let second_id = match &response.choices[1].blocks[0] {
            NeutralContentBlock::ToolCall { id, .. } => id,
            block => panic!("expected tool call, got {block:?}"),
        };
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn anthropic_response_wraps_single_choice_at_index_zero() {
        let adapter = AnthropicAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let body = json!({
            "id": "msg-1",
            "model": "claude-test",
            "content": [{ "type": "text", "text": "hello" }],
            "stop_reason": "max_tokens"
        })
        .to_string();

        let response = adapter.parse_response(200, &body, &upstream).unwrap();

        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].index, 0);
        assert_eq!(
            response.choices[0].finish_reason,
            Some(FinishReason::MaxTokens)
        );
        assert_eq!(
            response.choices[0].raw_finish_reason.as_deref(),
            Some("max_tokens")
        );
    }

    #[test]
    fn openai_stream_preserves_choice_and_tool_indexes() {
        let adapter = OpenAIAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);
        let data = json!({
            "id": "chat-stream",
            "model": "gpt-stream",
            "choices": [
                {
                    "index": 2,
                    "delta": {
                        "content": "hello",
                        "tool_calls": [{
                            "index": 4,
                            "id": "call-4",
                            "function": { "name": "lookup", "arguments": "{\"id\":" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                },
                { "index": 3, "delta": {}, "finish_reason": "length" }
            ],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 2,
                "total_tokens": 3
            }
        })
        .to_string();

        let events = decoder.decode_data(&data).unwrap();

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: Some("chat-stream".to_string()),
                    model: "gpt-stream".to_string(),
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 2,
                    text: "hello".to_string(),
                },
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 2,
                    tool_call_index: 4,
                    id: "call-4".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 2,
                    tool_call_index: 4,
                    arguments_delta: "{\"id\":".to_string(),
                },
                NeutralStreamEvent::UsageUpdate(UsageInfo {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                }),
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 2,
                    tool_call_index: 4,
                },
                NeutralStreamEvent::Finish {
                    choice_index: 2,
                    reason: FinishReason::ToolCall,
                    raw_finish_reason: Some("tool_calls".to_string()),
                },
                NeutralStreamEvent::Finish {
                    choice_index: 3,
                    reason: FinishReason::MaxTokens,
                    raw_finish_reason: Some("length".to_string()),
                },
            ]
        );
    }

    #[test]
    fn openai_stream_buffers_arguments_until_tool_metadata_arrives() {
        let adapter = OpenAIAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);

        let first_events = decoder
            .decode_data(
                &json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": 1,
                                "function": { "arguments": "{\"id\":" }
                            }]
                        },
                        "finish_reason": null
                    }]
                })
                .to_string(),
            )
            .unwrap();
        let second_events = decoder
            .decode_data(
                &json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": 1,
                                "id": "call-1",
                                "function": { "name": "lookup", "arguments": "1}" }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                })
                .to_string(),
            )
            .unwrap();

        assert_eq!(
            first_events,
            vec![NeutralStreamEvent::ResponseStart {
                response_id: None,
                model: "test-model".to_string(),
            }]
        );
        assert_eq!(
            second_events,
            vec![
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 0,
                    tool_call_index: 1,
                    id: "call-1".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: 1,
                    arguments_delta: "{\"id\":".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: 1,
                    arguments_delta: "1}".to_string(),
                },
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 0,
                    tool_call_index: 1,
                },
                NeutralStreamEvent::Finish {
                    choice_index: 0,
                    reason: FinishReason::ToolCall,
                    raw_finish_reason: Some("tool_calls".to_string()),
                },
            ]
        );
    }

    #[test]
    fn openai_done_marker_only_ends_response() {
        let adapter = OpenAIAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);

        assert_eq!(
            decoder.decode_data("[DONE]").unwrap(),
            vec![NeutralStreamEvent::ResponseEnd]
        );
        assert!(decoder.finish().unwrap().is_empty());
    }

    #[test]
    fn anthropic_stream_uses_content_block_index() {
        let adapter = AnthropicAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);
        let mut events = Vec::new();
        for data in [
            json!({
                "type": "message_start",
                "message": {
                    "id": "message-1",
                    "model": "claude-stream",
                    "usage": { "input_tokens": 2, "output_tokens": 0 }
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 6,
                "content_block": {
                    "type": "tool_use",
                    "id": "tool-6",
                    "name": "lookup",
                    "input": {}
                }
            }),
            json!({
                "type": "content_block_delta",
                "index": 6,
                "delta": { "type": "input_json_delta", "partial_json": "{\"q\":1}" }
            }),
            json!({ "type": "content_block_stop", "index": 6 }),
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "tool_use" },
                "usage": { "output_tokens": 9 }
            }),
            json!({ "type": "message_stop" }),
        ] {
            events.extend(decoder.decode_data(&data.to_string()).unwrap());
        }

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: Some("message-1".to_string()),
                    model: "claude-stream".to_string(),
                },
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 0,
                    tool_call_index: 6,
                    id: "tool-6".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: 6,
                    arguments_delta: "{\"q\":1}".to_string(),
                },
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 0,
                    tool_call_index: 6,
                },
                NeutralStreamEvent::UsageUpdate(UsageInfo {
                    prompt_tokens: 2,
                    completion_tokens: 9,
                    total_tokens: 11,
                }),
                NeutralStreamEvent::Finish {
                    choice_index: 0,
                    reason: FinishReason::ToolCall,
                    raw_finish_reason: Some("tool_use".to_string()),
                },
                NeutralStreamEvent::ResponseEnd,
            ]
        );
    }

    #[test]
    fn anthropic_stream_rejects_delta_for_unopened_block() {
        let adapter = AnthropicAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);
        decoder
            .decode_data(
                &json!({
                    "type": "message_start",
                    "message": { "id": "message-1", "model": "claude-stream" }
                })
                .to_string(),
            )
            .unwrap();

        let error = decoder
            .decode_data(
                &json!({
                    "type": "content_block_delta",
                    "index": 3,
                    "delta": { "type": "input_json_delta", "partial_json": "{}" }
                })
                .to_string(),
            )
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
    }

    #[test]
    fn gemini_stream_uses_candidate_and_part_indexes() {
        let adapter = GeminiAdapter::new();
        let upstream = create_upstream_model(ReasoningCapability::default());
        let mut decoder = adapter.create_stream_decoder(&upstream);
        let data = json!({
            "responseId": "gemini-stream",
            "candidates": [{
                "index": 5,
                "content": { "parts": [
                    { "text": "hello" },
                    { "functionCall": { "name": "lookup", "args": { "id": 1 } } }
                ] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 2,
                "candidatesTokenCount": 3,
                "totalTokenCount": 5
            }
        })
        .to_string();

        let mut events = decoder.decode_data(&data).unwrap();
        events.extend(decoder.finish().unwrap());

        assert_eq!(
            events,
            vec![
                NeutralStreamEvent::ResponseStart {
                    response_id: Some("gemini-stream".to_string()),
                    model: "test-model".to_string(),
                },
                NeutralStreamEvent::TextDelta {
                    choice_index: 5,
                    text: "hello".to_string(),
                },
                NeutralStreamEvent::ToolCallStart {
                    choice_index: 5,
                    tool_call_index: 1,
                    id: "call_5_1".to_string(),
                    name: "lookup".to_string(),
                },
                NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 5,
                    tool_call_index: 1,
                    arguments_delta: "{\"id\":1}".to_string(),
                },
                NeutralStreamEvent::ToolCallEnd {
                    choice_index: 5,
                    tool_call_index: 1,
                },
                NeutralStreamEvent::UsageUpdate(UsageInfo {
                    prompt_tokens: 2,
                    completion_tokens: 3,
                    total_tokens: 5,
                }),
                NeutralStreamEvent::Finish {
                    choice_index: 5,
                    reason: FinishReason::Stop,
                    raw_finish_reason: Some("STOP".to_string()),
                },
                NeutralStreamEvent::ResponseEnd,
            ]
        );
    }
}
