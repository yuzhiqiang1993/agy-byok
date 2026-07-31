#[cfg(test)]
mod tests {
    use crate::antigravity::{
        AntigravityModelDescriptor, AntigravityRequestParser, AntigravityResponseEncoder,
        AntigravityStreamEncoder,
    };
    use crate::domain::*;
    use crate::proxy::ProxyServer;
    use crate::storage::{AppConfig, ConfigStore};
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};

    fn connection_test_config(generate_endpoint: String) -> AppConfig {
        AppConfig {
            proxy_port: 51234,
            providers: vec![Provider {
                id: "p-connection".to_string(),
                name: "Connection Provider".to_string(),
                protocol: ProviderProtocol::Openai,
                models_endpoint: String::new(),
                generate_endpoint,
                api_key: "sk-connection".to_string(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 3000,
                request_timeout_ms: 5000,
                stream_idle_timeout_ms: 5000,
                enabled: true,
            }],
            upstream_models: vec![UpstreamModel {
                id: "um-connection".to_string(),
                provider_id: "p-connection".to_string(),
                upstream_model_id: "gpt-test".to_string(),
                display_name: "Connection Model".to_string(),
                capabilities: ModelCapabilities::default(),
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            virtual_models: vec![VirtualModel {
                id: "vm-connection".to_string(),
                host_model_id: None,
                upstream_model_id: "um-connection".to_string(),
                display_name: "Connection Model".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        }
    }

    #[tokio::test]
    async fn model_connection_test_sends_minimal_authenticated_request() {
        let response = json!({
            "id": "chatcmpl-connection",
            "model": "gpt-test",
            "choices": [{
                "message": { "role": "assistant", "content": "OK" },
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let (mock_url, _handle, recorded) =
            MockProviderServer::start_recording(200, &response).await;
        let server = ProxyServer::new(
            ConfigStore::in_memory(connection_test_config(format!("{mock_url}/v1/chat"))),
            0,
        );

        server.test_model_connection("vm-connection").await.unwrap();

        let recorded = recorded.await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&recorded.body).unwrap();
        assert_eq!(
            recorded.authorization.as_deref(),
            Some("Bearer sk-connection")
        );
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["max_tokens"], 8);
        assert_eq!(body["stream"], false);
    }

    #[tokio::test]
    async fn model_connection_test_expands_model_endpoint_placeholder() {
        let response = json!({
            "id": "chatcmpl-placeholder",
            "model": "gpt-test",
            "choices": [{
                "message": { "role": "assistant", "content": "OK" },
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let (mock_url, _handle, recorded) =
            MockProviderServer::start_recording(200, &response).await;
        let server = ProxyServer::new(
            ConfigStore::in_memory(connection_test_config(format!(
                "{mock_url}/v1/models/{{model}}:generate"
            ))),
            0,
        );

        server.test_model_connection("vm-connection").await.unwrap();

        assert_eq!(
            recorded.await.unwrap().path_and_query,
            "/v1/models/gpt-test:generate"
        );
    }

    #[tokio::test]
    async fn model_connection_test_reports_authentication_failure() {
        let (mock_url, _handle) =
            MockProviderServer::start(401, r#"{"error":{"message":"unauthorized"}}"#).await;
        let server = ProxyServer::new(
            ConfigStore::in_memory(connection_test_config(format!("{mock_url}/v1/chat"))),
            0,
        );

        let error = server
            .test_model_connection("vm-connection")
            .await
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::Authentication);
        assert_eq!(error.status_code, 401);
    }

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
            api_key: "sk-mock-api-key".to_string(),
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
            host_model_id: None,
            upstream_model_id: "um-test".to_string(),
            display_name: "Test Virtual Model".to_string(),
            default_reasoning_level: None,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        };

        let config = AppConfig {
            proxy_port: 51234,
            providers: vec![provider],
            upstream_models: vec![upstream_model],
            virtual_models: vec![virtual_model],
        };

        let config_store = ConfigStore::in_memory(config);
        let server = ProxyServer::new(config_store, 0);

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
        assert_eq!(activities[0].upstream_model_id.as_deref(), Some("gpt-4o"));
        assert_eq!(activities[0].provider_id, "p-test");
        assert_eq!(activities[0].provider_protocol.as_deref(), Some("openai"));
        assert_eq!(activities[0].status_code, 200);
        assert!(!activities[0].stream);
        assert_eq!(activities[0].message_count, 1);
        assert_eq!(activities[0].tool_count, 0);
        assert!(!activities[0].used_fallback);
        assert!(activities[0].error_detail.is_none());
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
            proxy_port: 51234,
            providers: vec![Provider {
                id: "p-stream".to_string(),
                name: "Mock Stream Provider".to_string(),
                protocol: ProviderProtocol::Openai,
                models_endpoint: format!("{mock_url}/v1/models"),
                generate_endpoint: format!("{mock_url}/v1/chat/completions"),
                api_key: "sk-stream".to_string(),
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
                host_model_id: None,
                upstream_model_id: "um-stream".to_string(),
                display_name: "Stream Model".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        };
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);
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
        assert!(activities[0].stream);
        assert_eq!(activities[0].message_count, 1);
    }

    #[test]
    fn model_list_injection_reports_reasoning_capability() {
        let config = AppConfig {
            proxy_port: 51234,
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
                host_model_id: Some("MODEL_PLACEHOLDER_M400".to_string()),
                upstream_model_id: "um-1".to_string(),
                display_name: "Claude 3.5 Sonnet BYOK".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        };

        let config_store = ConfigStore::in_memory(config);
        let server = ProxyServer::new(config_store, 0);

        let base_models_json = json!({
            "models": [
                {"id": "gemini-pro", "displayName": "Gemini Pro"}
            ],
            "agentModelSorts": [{
                "groups": [{
                    "modelIds": ["gemini-pro"]
                }]
            }]
        });

        let injected = server.handle_model_list(base_models_json);
        let models_arr = injected["models"].as_array().unwrap();
        assert_eq!(models_arr.len(), 2);
        assert_eq!(models_arr[1]["id"], "vm-claude");
        assert_eq!(models_arr[1]["supportsThinking"], true);
        assert_eq!(
            injected["agentModelSorts"][0]["groups"][0]["modelIds"],
            json!(["gemini-pro"])
        );
    }

    #[test]
    fn object_model_catalog_updates_valid_sorts_without_breaking_invalid_sorts() {
        let upstream_models = vec![UpstreamModel {
            id: "upstream-1".to_string(),
            provider_id: "provider-1".to_string(),
            upstream_model_id: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            capabilities: ModelCapabilities::default(),
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        }];
        let virtual_models = vec![
            VirtualModel {
                id: "virtual-1".to_string(),
                host_model_id: None,
                upstream_model_id: "upstream-1".to_string(),
                display_name: "Virtual One".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            },
            VirtualModel {
                id: "custom-virtual-2".to_string(),
                host_model_id: None,
                upstream_model_id: "upstream-1".to_string(),
                display_name: "Virtual Two".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            },
        ];
        let mut catalog = json!({
            "catalogVersion": "v10",
            "models": {
                "native-model": {
                    "displayName": "Native Model",
                    "model": "MODEL_NATIVE"
                }
            },
            "agentModelSorts": [{
                "sortId": "native-sort",
                "nativeField": true,
                "groups": [
                    {
                        "groupId": "already-listed",
                        "modelIds": ["native-model", "custom-virtual-1"]
                    },
                    {
                        "groupId": "append-custom",
                        "modelIds": ["native-secondary"]
                    }
                ]
            }]
        });

        AntigravityModelDescriptor::inject_into_model_list(
            &mut catalog,
            &virtual_models,
            &upstream_models,
        );
        AntigravityModelDescriptor::inject_into_model_list(
            &mut catalog,
            &virtual_models,
            &upstream_models,
        );

        assert_eq!(catalog["catalogVersion"], "v10");
        assert_eq!(catalog["models"].as_object().unwrap().len(), 3);
        assert_eq!(catalog["models"]["native-model"]["model"], "MODEL_NATIVE");
        assert!(catalog["models"]["custom-virtual-1"].is_object());
        assert!(catalog["models"]["custom-virtual-2"].is_object());
        assert_eq!(catalog["agentModelSorts"][0]["sortId"], "native-sort");
        assert_eq!(catalog["agentModelSorts"][0]["nativeField"], true);
        assert_eq!(
            catalog["agentModelSorts"][0]["groups"][0]["modelIds"],
            json!(["native-model", "custom-virtual-1", "custom-virtual-2"])
        );
        assert_eq!(
            catalog["agentModelSorts"][0]["groups"][1]["modelIds"],
            json!(["native-secondary", "custom-virtual-1", "custom-virtual-2"])
        );

        for mut malformed_catalog in [
            json!({
                "models": {
                    "native-model": { "model": "MODEL_NATIVE" }
                }
            }),
            json!({
                "models": {
                    "native-model": { "model": "MODEL_NATIVE" }
                },
                "agentModelSorts": { "groups": [] }
            }),
            json!({
                "models": {
                    "native-model": { "model": "MODEL_NATIVE" }
                },
                "agentModelSorts": [
                    { "groups": "invalid" },
                    { "groups": [{ "modelIds": "invalid" }, null] }
                ]
            }),
        ] {
            let original_sorts = malformed_catalog.get("agentModelSorts").cloned();
            AntigravityModelDescriptor::inject_into_model_list(
                &mut malformed_catalog,
                &virtual_models,
                &upstream_models,
            );

            assert_eq!(
                malformed_catalog["models"]["native-model"]["model"],
                "MODEL_NATIVE"
            );
            assert!(malformed_catalog["models"]["custom-virtual-1"].is_object());
            assert!(malformed_catalog["models"]["custom-virtual-2"].is_object());
            assert_eq!(
                malformed_catalog.get("agentModelSorts"),
                original_sorts.as_ref()
            );
        }

        let mut mixed_catalog = json!({
            "models": {
                "native-model": { "model": "MODEL_NATIVE" }
            },
            "agentModelSorts": [
                {
                    "sortId": "malformed-sort",
                    "groups": "invalid"
                },
                {
                    "sortId": "valid-sort",
                    "groups": [{
                        "groupId": "preserved-group",
                        "modelIds": ["native-model"]
                    }]
                }
            ]
        });
        let malformed_sort = mixed_catalog["agentModelSorts"][0].clone();

        AntigravityModelDescriptor::inject_into_model_list(
            &mut mixed_catalog,
            &virtual_models,
            &upstream_models,
        );

        assert_eq!(mixed_catalog["agentModelSorts"][0], malformed_sort);
        assert_eq!(mixed_catalog["agentModelSorts"][1]["sortId"], "valid-sort");
        assert_eq!(
            mixed_catalog["agentModelSorts"][1]["groups"][0]["groupId"],
            "preserved-group"
        );
        assert_eq!(
            mixed_catalog["agentModelSorts"][1]["groups"][0]["modelIds"],
            json!(["native-model", "custom-virtual-1", "custom-virtual-2"])
        );
    }

    #[test]
    fn antigravity_request_parser_extracts_nested_model_id_variants() {
        let variants = [
            json!({ "request": { "requestedModel": "models/MODEL_PLACEHOLDER_M400" } }),
            json!({ "request": { "planModel": "MODEL_PLACEHOLDER_M400" } }),
            json!({ "request": { "requested_model": "MODEL_PLACEHOLDER_M400" } }),
            json!({ "request": { "plan_model": "MODEL_PLACEHOLDER_M400" } }),
            json!({ "request": { "modelId": "MODEL_PLACEHOLDER_M400" } }),
            json!({ "request": { "model_id": "MODEL_PLACEHOLDER_M400" } }),
        ];

        for body in variants {
            assert_eq!(
                AntigravityRequestParser::extract_model_id(&body.to_string()).unwrap(),
                "MODEL_PLACEHOLDER_M400"
            );
        }
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
    fn antigravity_request_parser_pairs_tool_results_with_function_calls() {
        let body = json!({
            "model": "vm-1",
            "contents": [
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "call-explicit",
                            "name": "view_file",
                            "args": "{\"path\":\"src/main.rs\"}"
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "id": "call-explicit",
                            "name": "view_file",
                            "response": "file contents"
                        }
                    }]
                }
            ]
        })
        .to_string();

        let request = AntigravityRequestParser::parse(&body).unwrap();

        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, MessageRole::Assistant);
        assert_eq!(request.messages[1].role, MessageRole::Tool);
        assert_eq!(
            request.messages[0].blocks[0],
            NeutralContentBlock::ToolCall {
                id: "call-explicit".to_string(),
                name: "view_file".to_string(),
                arguments_json: "{\"path\":\"src/main.rs\"}".to_string(),
            }
        );
        assert_eq!(
            request.messages[1].blocks[0],
            NeutralContentBlock::ToolResult {
                tool_call_id: "call-explicit".to_string(),
                content: "file contents".to_string(),
            }
        );
    }

    #[test]
    fn antigravity_request_parser_matches_missing_ids_by_function_order() {
        let body = json!({
            "model": "vm-1",
            "contents": [
                {
                    "role": "model",
                    "parts": [
                        { "functionCall": { "name": "view_file", "args": { "path": "a" } } },
                        { "functionCall": { "name": "view_file", "args": { "path": "b" } } }
                    ]
                },
                {
                    "role": "user",
                    "parts": [
                        { "functionResponse": { "name": "view_file", "response": { "ok": "a" } } },
                        { "functionResponse": { "name": "view_file", "response": { "ok": "b" } } }
                    ]
                }
            ]
        })
        .to_string();

        let request = AntigravityRequestParser::parse(&body).unwrap();

        assert_eq!(request.messages.len(), 3);
        let result_ids = request.messages[1..]
            .iter()
            .map(|message| match &message.blocks[0] {
                NeutralContentBlock::ToolResult { tool_call_id, .. } => tool_call_id.as_str(),
                block => panic!("expected tool result, got {block:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(result_ids, vec!["call_0_0", "call_0_1"]);
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
