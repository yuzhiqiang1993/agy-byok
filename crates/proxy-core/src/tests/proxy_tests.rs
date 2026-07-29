#[cfg(test)]
mod tests {
    use crate::antigravity::{
        AntigravityRequestParser, AntigravityResponseEncoder, AntigravityStreamEncoder,
    };
    use crate::domain::*;
    use crate::proxy::ProxyServer;
    use crate::storage::{AppConfig, ConfigStore, KeyStore, MemoryKeyStore};
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;

    #[tokio::test]
    async fn proxy_server_handles_end_to_end_chat() {
        let mock_body = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello from Mock OpenAI!"
                },
                "finish_reason": "stop"
            }]
        })
        .to_string();

        let (mock_url, _handle) = MockProviderServer::start(200, &mock_body).await;

        let provider = Provider {
            id: "p-test".to_string(),
            name: "Mock Provider".to_string(),
            protocol: ProviderProtocol::Openai,
            models_endpoint: format!("{}/v1/models", mock_url),
            generate_endpoint: format!("{}/v1/chat/completions", mock_url),
            api_key_ref: "key-ref-test".to_string(),
            headers: HashMap::new(),
            default_parameters: ParameterOverrides::default(),
            connect_timeout_ms: 3000,
            request_timeout_ms: 5000,
            stream_idle_timeout_ms: 5000,
            enabled: true,
        };

        let upstream_model = UpstreamModel {
            id: "um-test".to_string(),
            provider_id: "p-test".to_string(),
            upstream_model_id: "gpt-4o".to_string(),
            display_name: "GPT-4o".to_string(),
            capabilities: ModelCapabilities::default(),
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        };

        let virtual_model = VirtualModel {
            id: "vm-test-1".to_string(),
            upstream_model_id: "um-test".to_string(),
            display_name: "Test Virtual Model".to_string(),
            default_reasoning_level: None,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        };

        let config = AppConfig {
            providers: vec![provider],
            upstream_models: vec![upstream_model],
            virtual_models: vec![virtual_model],
        };

        let config_store = ConfigStore::in_memory(config);
        let key_store = Arc::new(MemoryKeyStore::new());
        key_store
            .set_secret("key-ref-test", "sk-mock-api-key")
            .await
            .unwrap();

        let server = ProxyServer::new(config_store, key_store, 0);

        let request = NeutralChatRequest {
            virtual_model_id: "vm-test-1".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Test message".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let response = server.handle_chat_request(&request).await.unwrap();
        assert!(response.contains("Hello from Mock OpenAI!"));

        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].virtual_model_id, "vm-test-1");
        assert_eq!(activities[0].status_code, 200);
    }

    #[tokio::test]
    async fn proxy_server_streams_chunked_sse_with_complete_tool_call() {
        let upstream_sse = format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({
                "id": "chat-stream-1",
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "delta": { "content": "你" },
                    "finish_reason": null
                }]
            }),
            json!({
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call-9",
                            "function": { "name": "lookup", "arguments": "{\"id\":" }
                        }]
                    },
                    "finish_reason": null
                }]
            }),
            json!({
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": { "arguments": "1}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 2,
                    "completion_tokens": 3,
                    "total_tokens": 5
                }
            })
        );
        let unicode_offset = upstream_sse.find('你').unwrap();
        let upstream_bytes = upstream_sse.into_bytes();
        let chunks = vec![
            upstream_bytes[..unicode_offset + 1].to_vec(),
            upstream_bytes[unicode_offset + 1..unicode_offset + 2].to_vec(),
            upstream_bytes[unicode_offset + 2..].to_vec(),
        ];
        let (mock_url, _handle) = MockProviderServer::start_chunked(200, chunks).await;

        let config = AppConfig {
            providers: vec![Provider {
                id: "p-stream".to_string(),
                name: "Mock Stream Provider".to_string(),
                protocol: ProviderProtocol::Openai,
                models_endpoint: format!("{mock_url}/v1/models"),
                generate_endpoint: format!("{mock_url}/v1/chat/completions"),
                api_key_ref: "key-ref-stream".to_string(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 3000,
                request_timeout_ms: 5000,
                stream_idle_timeout_ms: 5000,
                enabled: true,
            }],
            upstream_models: vec![UpstreamModel {
                id: "um-stream".to_string(),
                provider_id: "p-stream".to_string(),
                upstream_model_id: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                capabilities: ModelCapabilities {
                    vision: false,
                    tools: true,
                    reasoning: ReasoningCapability::default(),
                },
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            virtual_models: vec![VirtualModel {
                id: "vm-stream".to_string(),
                upstream_model_id: "um-stream".to_string(),
                display_name: "Stream Model".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        };
        let key_store = Arc::new(MemoryKeyStore::new());
        key_store
            .set_secret("key-ref-stream", "sk-stream")
            .await
            .unwrap();
        let server = ProxyServer::new(ConfigStore::in_memory(config), key_store, 0);
        let request = NeutralChatRequest {
            virtual_model_id: "vm-stream".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("stream".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            reasoning_level: None,
            stream: true,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let response = server.handle_chat_request(&request).await.unwrap();

        assert!(response.contains('你'));
        assert!(response.contains("\"id\":\"call-9\""));
        assert!(response.contains("\"args\":{\"id\":1}"));
        assert_eq!(response.matches("data: [DONE]").count(), 1);
        assert!(
            response.find("\"functionCall\"").unwrap()
                < response.find("\"finishReason\":\"TOOL_CALL\"").unwrap()
        );
        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].status_code, 200);
    }

    #[test]
    fn model_list_injection_reports_reasoning_capability() {
        let config = AppConfig {
            providers: vec![],
            upstream_models: vec![UpstreamModel {
                id: "um-1".to_string(),
                provider_id: "p-1".to_string(),
                upstream_model_id: "claude-3-5-sonnet".to_string(),
                display_name: "Claude 3.5".to_string(),
                capabilities: ModelCapabilities {
                    vision: true,
                    tools: true,
                    reasoning: ReasoningCapability {
                        levels: BTreeMap::from([(
                            ReasoningLevel::High,
                            ReasoningMapping::BudgetTokens(4096),
                        )]),
                    },
                },
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            virtual_models: vec![VirtualModel {
                id: "vm-claude".to_string(),
                upstream_model_id: "um-1".to_string(),
                display_name: "Claude 3.5 Sonnet BYOK".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        };

        let config_store = ConfigStore::in_memory(config);
        let key_store = Arc::new(MemoryKeyStore::new());
        let server = ProxyServer::new(config_store, key_store, 0);

        let base_models_json = json!({
            "models": [
                {"id": "gemini-pro", "displayName": "Gemini Pro"}
            ]
        });

        let injected = server.handle_model_list(base_models_json);
        let models_arr = injected["models"].as_array().unwrap();
        assert_eq!(models_arr.len(), 2);
        assert_eq!(models_arr[1]["id"], "vm-claude");
        assert_eq!(models_arr[1]["supportsThinking"], true);
    }

    #[test]
    fn antigravity_request_parser_preserves_thinking_and_unique_tool_ids() {
        let body = json!({
            "model": "vm-1",
            "contents": [{
                "role": "model",
                "parts": [
                    { "thought": true, "text": "internal reasoning" },
                    { "functionCall": { "name": "lookup", "args": { "id": 1 } } },
                    { "functionCall": { "name": "lookup", "args": { "id": 2 } } }
                ]
            }]
        })
        .to_string();

        let request = AntigravityRequestParser::parse(&body).unwrap();

        assert_eq!(request.reasoning_level, None);
        assert_eq!(
            request.messages[0].blocks[0],
            NeutralContentBlock::Thinking {
                text: "internal reasoning".to_string(),
                signature: None,
            }
        );
        let first_id = match &request.messages[0].blocks[1] {
            NeutralContentBlock::ToolCall { id, .. } => id,
            block => panic!("expected tool call, got {block:?}"),
        };
        let second_id = match &request.messages[0].blocks[2] {
            NeutralContentBlock::ToolCall { id, .. } => id,
            block => panic!("expected tool call, got {block:?}"),
        };
        assert_eq!(first_id, "call_0_1");
        assert_eq!(second_id, "call_0_2");
    }

    #[test]
    fn antigravity_response_encoder_emits_all_candidates() {
        let response = NeutralChatResponse {
            id: "response-1".to_string(),
            model: "test-model".to_string(),
            choices: vec![
                NeutralChoice {
                    index: 2,
                    blocks: vec![NeutralContentBlock::Text("first".to_string())],
                    finish_reason: Some(FinishReason::Stop),
                    raw_finish_reason: Some("stop".to_string()),
                },
                NeutralChoice {
                    index: 7,
                    blocks: vec![NeutralContentBlock::Thinking {
                        text: "thinking".to_string(),
                        signature: None,
                    }],
                    finish_reason: Some(FinishReason::MaxTokens),
                    raw_finish_reason: Some("length".to_string()),
                },
                NeutralChoice {
                    index: 9,
                    blocks: vec![],
                    finish_reason: Some(FinishReason::ToolCall),
                    raw_finish_reason: Some("tool_calls".to_string()),
                },
            ],
            usage: Some(UsageInfo {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
            }),
        };

        let encoded: serde_json::Value =
            serde_json::from_str(&AntigravityResponseEncoder::encode_response(&response)).unwrap();

        assert_eq!(encoded["candidates"].as_array().unwrap().len(), 3);
        assert_eq!(encoded["candidates"][0]["index"], 2);
        assert_eq!(encoded["candidates"][0]["finishReason"], "STOP");
        assert_eq!(encoded["candidates"][1]["index"], 7);
        assert_eq!(encoded["candidates"][1]["finishReason"], "MAX_TOKENS");
        assert_eq!(encoded["candidates"][2]["index"], 9);
        assert_eq!(encoded["candidates"][2]["finishReason"], "TOOL_CALL");
        assert_eq!(
            encoded["candidates"][1]["content"]["parts"][0]["thought"],
            true
        );
    }

    #[test]
    fn antigravity_stream_encoder_waits_for_complete_tool_arguments() {
        let mut encoder = AntigravityStreamEncoder::new();

        assert!(encoder
            .encode_event(&NeutralStreamEvent::ToolCallStart {
                choice_index: 0,
                tool_call_index: 1,
                id: "call-1".to_string(),
                name: "lookup".to_string(),
            })
            .unwrap()
            .is_empty());
        assert!(encoder
            .encode_event(&NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index: 0,
                tool_call_index: 1,
                arguments_delta: "{\"id\":".to_string(),
            })
            .unwrap()
            .is_empty());
        assert!(encoder
            .encode_event(&NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index: 0,
                tool_call_index: 1,
                arguments_delta: "1}".to_string(),
            })
            .unwrap()
            .is_empty());

        let frames = encoder
            .encode_event(&NeutralStreamEvent::ToolCallEnd {
                choice_index: 0,
                tool_call_index: 1,
            })
            .unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(frames[0].strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(
            payload["candidates"][0]["content"]["parts"][0]["functionCall"]["id"],
            "call-1"
        );
        assert_eq!(
            payload["candidates"][0]["content"]["parts"][0]["functionCall"]["args"]["id"],
            1
        );
        assert_eq!(
            encoder
                .encode_event(&NeutralStreamEvent::ResponseEnd)
                .unwrap(),
            vec!["data: [DONE]\n\n".to_string()]
        );
        assert!(encoder
            .encode_event(&NeutralStreamEvent::ResponseEnd)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn antigravity_stream_encoder_rejects_invalid_tool_arguments() {
        let mut encoder = AntigravityStreamEncoder::new();
        encoder
            .encode_event(&NeutralStreamEvent::ToolCallStart {
                choice_index: 0,
                tool_call_index: 0,
                id: "call-1".to_string(),
                name: "lookup".to_string(),
            })
            .unwrap();
        encoder
            .encode_event(&NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index: 0,
                tool_call_index: 0,
                arguments_delta: "{\"id\":".to_string(),
            })
            .unwrap();

        let error = encoder
            .encode_event(&NeutralStreamEvent::ToolCallEnd {
                choice_index: 0,
                tool_call_index: 0,
            })
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
    }
}
