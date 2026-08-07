#[cfg(test)]
mod tests {
    use crate::antigravity::{
        AntigravityModelDescriptor, AntigravityRequestParser, AntigravityResponseEncoder,
        AntigravityStreamEncoder,
    };
    use crate::domain::*;
    use crate::proxy::ProxyServer;
    use crate::routing::RouteTable;
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
                protocol: ProviderProtocol::OpenaiChatCompletions,
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
                token_limits: ModelTokenLimits::default(),
                checkpoint_override: None,
                tokenizer: None,
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
            official_model_settings: OfficialModelSettings::default(),
        }
    }

    fn fallback_config(primary_endpoint: String, fallback_endpoint: String) -> AppConfig {
        let mut config = connection_test_config(primary_endpoint);
        config.providers[0].id = "p-primary".to_string();
        config.providers[0].name = "Primary Provider".to_string();
        config.upstream_models[0].id = "um-primary".to_string();
        config.upstream_models[0].provider_id = "p-primary".to_string();
        config.upstream_models[0].upstream_model_id = "primary-model".to_string();
        config.virtual_models[0].id = "vm-primary".to_string();
        config.virtual_models[0].host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());
        config.virtual_models[0].upstream_model_id = "um-primary".to_string();
        config.virtual_models[0].fallback_virtual_model_id = Some("vm-fallback".to_string());

        let mut fallback_provider = config.providers[0].clone();
        fallback_provider.id = "p-fallback".to_string();
        fallback_provider.name = "Fallback Provider".to_string();
        fallback_provider.generate_endpoint = fallback_endpoint;
        config.providers.push(fallback_provider);

        let mut fallback_upstream = config.upstream_models[0].clone();
        fallback_upstream.id = "um-fallback".to_string();
        fallback_upstream.provider_id = "p-fallback".to_string();
        fallback_upstream.upstream_model_id = "fallback-model".to_string();
        config.upstream_models.push(fallback_upstream);

        let mut fallback_virtual = config.virtual_models[0].clone();
        fallback_virtual.id = "vm-fallback".to_string();
        fallback_virtual.host_model_id = Some("MODEL_PLACEHOLDER_M401".to_string());
        fallback_virtual.upstream_model_id = "um-fallback".to_string();
        fallback_virtual.fallback_virtual_model_id = None;
        config.virtual_models.push(fallback_virtual);
        config
    }

    fn chat_request(virtual_model_id: &str) -> NeutralChatRequest {
        NeutralChatRequest {
            virtual_model_id: virtual_model_id.to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("hello".to_string())],
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
    fn injected_catalog_key_resolves_to_the_same_virtual_model() {
        let config = connection_test_config("http://localhost/chat".to_string());
        let mut catalog = json!({ "models": {} });
        AntigravityModelDescriptor::inject_into_model_list(
            &mut catalog,
            &config.virtual_models,
            &config.upstream_models,
        );
        let catalog_key = catalog["models"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        let request = NeutralChatRequest {
            virtual_model_id: catalog_key.clone(),
            messages: vec![],
            system_instruction: None,
            tools: vec![],
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let route = RouteTable::resolve(&config, &request).unwrap();

        assert_eq!(catalog_key, config.virtual_models[0].catalog_key());
        assert_eq!(route.virtual_model.id, config.virtual_models[0].id);
    }

    #[test]
    fn model_catalog_applies_custom_checkpoint_settings() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.upstream_models[0].token_limits = ModelTokenLimits {
            context_window: Some(372_000),
            input_token_limit: Some(372_000),
            output_token_limit: Some(128_000),
            ..ModelTokenLimits::default()
        };
        config
            .official_model_settings
            .custom_model_threshold_percent = Some(80);
        let catalog_key = config.virtual_models[0].catalog_key().into_owned();
        let checkpoint_model = config.virtual_models[0]
            .effective_host_model_id()
            .into_owned();
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let catalog = server.handle_model_list(json!({ "models": {} }));
        let raw = catalog["models"][catalog_key]["modelExperiments"]["experiments"]
            ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"]
            .as_str()
            .expect("custom model must contain checkpoint settings");
        let checkpoint: serde_json::Value = serde_json::from_str(raw).unwrap();

        assert_eq!(checkpoint["token_threshold"], "297600");
        assert_eq!(checkpoint["max_token_limit"], "372000");
        assert_eq!(checkpoint["max_output_tokens"], "16384");
        assert_eq!(checkpoint["checkpoint_model"], checkpoint_model);
    }

    #[test]
    fn model_custom_checkpoint_override_does_not_change_official_gemini() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.upstream_models[0].token_limits = ModelTokenLimits {
            context_window: Some(372_000),
            input_token_limit: Some(372_000),
            output_token_limit: Some(128_000),
            ..ModelTokenLimits::default()
        };
        config.upstream_models[0].checkpoint_override = Some(ModelCheckpointOverride::Custom {
            token_threshold: 250_000,
            max_token_limit: 300_000,
            max_output_tokens: 20_000,
        });
        config.official_model_settings = OfficialModelSettings {
            gemini_compression_profile: OfficialCompressionProfile::Safe,
            custom_model_threshold_percent: Some(60),
            ..OfficialModelSettings::default()
        };
        let catalog_key = config.virtual_models[0].catalog_key().into_owned();
        let checkpoint_model = config.virtual_models[0]
            .effective_host_model_id()
            .into_owned();
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let catalog = server.handle_model_list(json!({
            "models": {
                "gemini-pro": {
                    "model": "MODEL_GEMINI_2_5_PRO",
                    "displayName": "Gemini Pro"
                }
            }
        }));
        let custom_raw = catalog["models"][catalog_key]["modelExperiments"]["experiments"]
            ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"]
            .as_str()
            .expect("custom model must contain checkpoint settings");
        let custom_checkpoint: serde_json::Value = serde_json::from_str(custom_raw).unwrap();
        let official_raw = catalog["models"]["gemini-pro"]["modelExperiments"]["experiments"]
            ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"]
            .as_str()
            .expect("official Gemini model must contain checkpoint settings");
        let official_checkpoint: serde_json::Value = serde_json::from_str(official_raw).unwrap();

        assert_eq!(custom_checkpoint["token_threshold"], "250000");
        assert_eq!(custom_checkpoint["max_token_limit"], "300000");
        assert_eq!(custom_checkpoint["max_output_tokens"], "20000");
        assert_eq!(custom_checkpoint["checkpoint_model"], checkpoint_model);
        assert_eq!(official_checkpoint["token_threshold"], "430000");
        assert_eq!(official_checkpoint["max_token_limit"], "512000");
        assert_eq!(official_checkpoint["max_output_tokens"], "16384");
    }

    #[test]
    fn fallback_inherits_request_parameters_and_extra_body() {
        let mut config = fallback_config(
            "http://localhost/primary".to_string(),
            "http://localhost/fallback".to_string(),
        );
        config.providers[1].default_parameters = ParameterOverrides {
            temperature: Some(0.1),
            extra_body: Some(HashMap::from([("provider_flag".to_string(), json!(true))])),
            ..ParameterOverrides::default()
        };
        config.upstream_models[1].parameter_overrides.top_p = Some(0.7);

        let mut request = chat_request("vm-primary");
        request.generation_parameters.temperature = Some(0.8);
        request.generation_parameters.max_tokens = Some(321);
        request.extra_body = HashMap::from([
            (
                "response_format".to_string(),
                json!({ "type": "json_object" }),
            ),
            ("model".to_string(), json!("must-not-override-route")),
        ]);
        let primary_route = RouteTable::resolve(&config, &request).unwrap();

        let fallback_route = RouteTable::resolve_fallback(&config, &primary_route, &request)
            .unwrap()
            .unwrap();

        assert_eq!(fallback_route.virtual_model.id, "vm-fallback");
        assert_eq!(fallback_route.final_parameters.temperature, Some(0.8));
        assert_eq!(fallback_route.final_parameters.max_tokens, Some(321));
        assert_eq!(fallback_route.final_parameters.top_p, Some(0.7));
        let extra_body = fallback_route.final_parameters.extra_body.unwrap();
        assert_eq!(extra_body["provider_flag"], true);
        assert_eq!(extra_body["response_format"]["type"], "json_object");
        assert!(!extra_body.contains_key("model"));
    }

    #[test]
    fn model_catalog_without_reasoning_uses_provider_only_name() {
        let config = connection_test_config("http://localhost/chat".to_string());
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let catalog = server.handle_model_list(json!({ "models": {} }));
        let model = &catalog["models"]["custom-vm-connection"];

        assert_eq!(
            model["displayName"],
            "Connection Model(Connection Provider)"
        );
        assert_eq!(model["supportsThinking"], false);
        assert!(model.get("reasoningEffort").is_none());
        assert!(model.get("thinkingBudget").is_none());
    }

    #[test]
    fn model_catalog_excludes_models_from_disabled_providers() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.providers[0].enabled = false;
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let catalog = server.handle_model_list(json!({ "models": {} }));

        assert!(catalog["models"].as_object().unwrap().is_empty());
    }

    #[test]
    fn model_catalog_includes_provider_name_and_default_reasoning_level() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.upstream_models[0].capabilities.reasoning = ReasoningCapability {
            levels: BTreeMap::from([(
                ReasoningLevel::High,
                ReasoningMapping::Effort("high".to_string()),
            )]),
        };
        config.virtual_models[0].default_reasoning_level = Some(ReasoningLevel::High);
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let catalog = server.handle_model_list(json!({ "models": {} }));
        let model = &catalog["models"]["custom-vm-connection"];

        assert_eq!(
            model["displayName"],
            "Connection Model high(Connection Provider)"
        );
        assert_eq!(model["supportsThinking"], true);
        assert_eq!(model["reasoningEffort"], "high");
    }

    #[test]
    fn model_catalog_expands_reasoning_variants_with_distinct_names() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.upstream_models[0].capabilities.reasoning = ReasoningCapability {
            levels: BTreeMap::from([
                (
                    ReasoningLevel::Low,
                    ReasoningMapping::Effort("low".to_string()),
                ),
                (
                    ReasoningLevel::High,
                    ReasoningMapping::Effort("high".to_string()),
                ),
            ]),
        };
        config.virtual_models[0].default_reasoning_level = Some(ReasoningLevel::Low);
        let mut high_model = config.virtual_models[0].clone();
        high_model.id = "vm-connection-high".to_string();
        high_model.host_model_id = Some("MODEL_PLACEHOLDER_M401".to_string());
        high_model.default_reasoning_level = Some(ReasoningLevel::High);
        config.virtual_models.push(high_model);

        let catalog = ProxyServer::new(ConfigStore::in_memory(config), 0)
            .handle_model_list(json!({ "models": {} }));

        assert_eq!(
            catalog["models"]["custom-vm-connection"]["displayName"],
            "Connection Model low(Connection Provider)"
        );
        assert_eq!(
            catalog["models"]["custom-vm-connection-high"]["displayName"],
            "Connection Model high(Connection Provider)"
        );
        assert_eq!(
            catalog["models"]["custom-vm-connection"]["reasoningEffort"],
            "low"
        );
        assert_eq!(
            catalog["models"]["custom-vm-connection-high"]["reasoningEffort"],
            "high"
        );
    }

    #[test]
    fn model_catalog_maps_budget_reasoning_preferences_to_ide_modes() {
        let mut config = connection_test_config("http://localhost/chat".to_string());
        config.upstream_models[0].capabilities.reasoning = ReasoningCapability {
            levels: BTreeMap::from([(ReasoningLevel::High, ReasoningMapping::BudgetTokens(8192))]),
        };

        let default_catalog = ProxyServer::new(ConfigStore::in_memory(config.clone()), 0)
            .handle_model_list(json!({ "models": {} }));
        let default_model = &default_catalog["models"]["custom-vm-connection"];
        assert!(default_model.get("reasoningEffort").is_none());
        assert!(default_model.get("thinkingBudget").is_none());

        config.virtual_models[0].default_reasoning_level = Some(ReasoningLevel::High);
        let high_catalog = ProxyServer::new(ConfigStore::in_memory(config), 0)
            .handle_model_list(json!({ "models": {} }));
        let high_model = &high_catalog["models"]["custom-vm-connection"];
        assert_eq!(high_model["reasoningEffort"], "high");
        assert_eq!(high_model["thinkingBudget"], 8192);
        assert!(high_model["thinkingBudget"].is_number());
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
    async fn model_connection_test_preserves_reasoning_mapping() {
        let response = json!({
            "id": "chatcmpl-reasoning-connection",
            "model": "gpt-test",
            "choices": [{
                "message": { "role": "assistant", "content": "OK" },
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let (mock_url, _handle, recorded) =
            MockProviderServer::start_recording(200, &response).await;
        let mut config = connection_test_config(format!("{mock_url}/v1/chat"));
        config.upstream_models[0].capabilities.reasoning = ReasoningCapability {
            levels: BTreeMap::from([(
                ReasoningLevel::High,
                ReasoningMapping::Effort("high".to_string()),
            )]),
        };
        config.virtual_models[0].default_reasoning_level = Some(ReasoningLevel::High);
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        server
            .test_model_connection_with_reasoning("vm-connection", ReasoningLevel::High)
            .await
            .unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&recorded.await.unwrap().body).unwrap();
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("max_tokens").is_none());
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
    async fn non_streaming_provider_response_enforces_buffer_limit() {
        let oversized_body =
            "x".repeat(crate::upstream_body::DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES + 1);
        let (mock_url, _handle) = MockProviderServer::start(200, &oversized_body).await;
        let server = ProxyServer::new(
            ConfigStore::in_memory(connection_test_config(format!("{mock_url}/v1/chat"))),
            0,
        );

        let error = server
            .handle_chat_request(&chat_request("vm-connection"))
            .await
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::UpstreamServerError);
        assert_eq!(error.status_code, 502);
        assert!(error.message.contains("exceeds"));
    }

    #[tokio::test]
    async fn activity_distinguishes_requested_entry_from_successful_fallback_route() {
        let (primary_url, _primary_handle) =
            MockProviderServer::start(500, r#"{"error":{"message":"primary failed"}}"#).await;
        let fallback_body = json!({
            "id": "chatcmpl-fallback",
            "model": "fallback-model",
            "choices": [{
                "message": { "role": "assistant", "content": "fallback response" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 2,
                "total_tokens": 7
            }
        })
        .to_string();
        let (fallback_url, _fallback_handle) = MockProviderServer::start(200, &fallback_body).await;
        let config = fallback_config(
            format!("{primary_url}/v1/chat"),
            format!("{fallback_url}/v1/chat"),
        );
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let response = server
            .handle_chat_request(&chat_request("MODEL_PLACEHOLDER_M400"))
            .await
            .unwrap();

        assert!(response.contains("fallback response"));
        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(
            activities[0].requested_virtual_model_id,
            "MODEL_PLACEHOLDER_M400"
        );
        assert_eq!(activities[0].virtual_model_id, "vm-fallback");
        assert_eq!(
            activities[0].upstream_model_id.as_deref(),
            Some("fallback-model")
        );
        assert!(activities[0].used_fallback);
        assert!(activities[0].fallback_attempted);
        assert!(activities[0].fallback_succeeded);
        assert_eq!(activities[0].input_tokens, Some(5));
        assert_eq!(activities[0].output_tokens, Some(2));
        assert_eq!(activities[0].total_tokens, Some(7));
    }

    #[tokio::test]
    async fn fallback_failure_is_returned_and_recorded_consistently() {
        let (primary_url, _primary_handle) =
            MockProviderServer::start(500, r#"{"error":{"message":"primary failed"}}"#).await;
        let (fallback_url, _fallback_handle) =
            MockProviderServer::start(401, r#"{"error":{"message":"fallback unauthorized"}}"#)
                .await;
        let config = fallback_config(
            format!("{primary_url}/v1/chat"),
            format!("{fallback_url}/v1/chat"),
        );
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let error = server
            .handle_chat_request(&chat_request("vm-primary"))
            .await
            .unwrap_err();

        assert_eq!(error.status_code, 401);
        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].virtual_model_id, "vm-fallback");
        assert_eq!(activities[0].status_code, 401);
        assert!(activities[0].fallback_attempted);
        assert!(!activities[0].fallback_succeeded);
        assert_eq!(
            activities[0].error_detail.as_deref(),
            Some("message=fallback unauthorized")
        );
    }

    #[tokio::test]
    async fn fallback_resolution_failure_is_visible_in_activity() {
        let (primary_url, _primary_handle) =
            MockProviderServer::start(500, r#"{"error":{"message":"primary failed"}}"#).await;
        let mut config = fallback_config(
            format!("{primary_url}/v1/chat"),
            "http://localhost/unused".to_string(),
        );
        config.virtual_models[1].enabled = false;
        let server = ProxyServer::new(ConfigStore::in_memory(config), 0);

        let error = server
            .handle_chat_request(&chat_request("vm-primary"))
            .await
            .unwrap_err();

        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].status_code, error.status_code);
        assert_eq!(activities[0].virtual_model_id, "vm-primary");
        assert!(activities[0].fallback_attempted);
        assert!(!activities[0].fallback_succeeded);
        assert!(activities[0]
            .error_detail
            .as_deref()
            .unwrap_or_default()
            .contains("vm-fallback"));
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
            }],
            "usage": {
                "prompt_tokens": 7,
                "completion_tokens": 4,
                "total_tokens": 11
            }
        })
        .to_string();

        let (mock_url, _handle) = MockProviderServer::start(200, &mock_body).await;

        let provider = Provider {
            id: "p-test".to_string(),
            name: "Mock Provider".to_string(),
            protocol: ProviderProtocol::OpenaiChatCompletions,
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
            token_limits: ModelTokenLimits::default(),
            checkpoint_override: None,
            tokenizer: None,
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
            official_model_settings: OfficialModelSettings::default(),
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
        assert_eq!(activities[0].requested_virtual_model_id, "vm-test-1");
        assert_eq!(activities[0].virtual_model_id, "vm-test-1");
        assert_eq!(activities[0].upstream_model_id.as_deref(), Some("gpt-4o"));
        assert_eq!(activities[0].provider_id, "p-test");
        assert_eq!(
            activities[0].provider_protocol.as_deref(),
            Some("openai_chat_completions")
        );
        assert_eq!(activities[0].status_code, 200);
        assert!(!activities[0].stream);
        assert_eq!(activities[0].message_count, 1);
        assert_eq!(activities[0].tool_count, 0);
        assert!(!activities[0].used_fallback);
        assert!(!activities[0].fallback_attempted);
        assert!(!activities[0].fallback_succeeded);
        assert_eq!(activities[0].input_tokens, Some(7));
        assert_eq!(activities[0].output_tokens, Some(4));
        assert_eq!(activities[0].total_tokens, Some(11));
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
                    "prompt_tokens": 12,
                    "completion_tokens": 9,
                    "total_tokens": 21,
                    "prompt_tokens_details": { "cached_tokens": 5 },
                    "completion_tokens_details": { "reasoning_tokens": 4 }
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
                protocol: ProviderProtocol::OpenaiChatCompletions,
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
                token_limits: ModelTokenLimits::default(),
                checkpoint_override: None,
                tokenizer: None,
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
            official_model_settings: OfficialModelSettings::default(),
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
        assert_eq!(response.matches("\"usageMetadata\"").count(), 1);
        assert!(response.contains("\"promptTokenCount\":12"));
        assert!(response.contains("\"candidatesTokenCount\":5"));
        assert!(response.contains("\"cachedContentTokenCount\":5"));
        assert!(response.contains("\"thoughtsTokenCount\":4"));
        assert!(
            response.find("\"functionCall\"").unwrap()
                < response.find("\"finishReason\":\"TOOL_CALL\"").unwrap()
        );
        assert!(
            response.find("\"usageMetadata\"").unwrap() < response.find("data: [DONE]").unwrap()
        );
        let activities = server.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].status_code, 200);
        assert!(activities[0].stream);
        assert_eq!(activities[0].message_count, 1);
        assert_eq!(activities[0].input_tokens, Some(7));
        assert_eq!(activities[0].output_tokens, Some(5));
        assert_eq!(activities[0].cache_read_tokens, Some(5));
        assert_eq!(activities[0].cache_write_tokens, None);
        assert_eq!(activities[0].reasoning_tokens, Some(4));
        assert_eq!(activities[0].total_tokens, Some(21));
    }

    #[test]
    fn model_list_injection_reports_reasoning_capability() {
        let config = AppConfig {
            proxy_port: 51234,
            providers: vec![Provider {
                id: "p-1".to_string(),
                name: "Anthropic".to_string(),
                protocol: ProviderProtocol::AnthropicMessages,
                models_endpoint: String::new(),
                generate_endpoint: "http://localhost/messages".to_string(),
                api_key: String::new(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 3000,
                request_timeout_ms: 5000,
                stream_idle_timeout_ms: 5000,
                enabled: true,
            }],
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
                token_limits: ModelTokenLimits::default(),
                checkpoint_override: None,
                tokenizer: None,
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
            official_model_settings: OfficialModelSettings::default(),
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
            token_limits: ModelTokenLimits::default(),
            checkpoint_override: None,
            tokenizer: None,
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
                    {
                        "thought": true,
                        "text": "internal reasoning",
                        "thoughtSignature": "signed-reasoning"
                    },
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
                signature: Some("signed-reasoning".to_string()),
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
    fn antigravity_request_parser_merges_streamed_thinking_signature_parts() {
        let body = json!({
            "model": "vm-1",
            "contents": [{
                "role": "model",
                "parts": [
                    { "thought": true, "text": "reason " },
                    {
                        "thought": true,
                        "text": "summary",
                        "thoughtSignature": "signed-thinking"
                    }
                ]
            }]
        })
        .to_string();

        let request = AntigravityRequestParser::parse(&body).unwrap();

        assert_eq!(
            request.messages[0].blocks,
            vec![NeutralContentBlock::Thinking {
                text: "reason summary".to_string(),
                signature: Some("signed-thinking".to_string()),
            }]
        );
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
                name: Some("view_file".to_string()),
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
                        signature: Some("signed-thinking".to_string()),
                    }],
                    finish_reason: Some(FinishReason::MaxTokens),
                    raw_finish_reason: Some("length".to_string()),
                },
                NeutralChoice {
                    index: 9,
                    blocks: vec![NeutralContentBlock::ToolCall {
                        id: "call-9".to_string(),
                        name: "lookup".to_string(),
                        arguments_json: r#"{"id":9}"#.to_string(),
                    }],
                    finish_reason: Some(FinishReason::ToolCall),
                    raw_finish_reason: Some("tool_calls".to_string()),
                },
            ],
            usage: Some(UsageInfo {
                input_tokens: 1,
                output_tokens: 2,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
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
            encoded["candidates"][2]["content"]["parts"][0]["functionCall"]["id"],
            "call-9"
        );
        assert_eq!(
            encoded["candidates"][1]["content"]["parts"][0]["thought"],
            true
        );
        assert_eq!(
            encoded["candidates"][1]["content"]["parts"][0]["thoughtSignature"],
            "signed-thinking"
        );
        assert_eq!(encoded["usageMetadata"]["promptTokenCount"], 1);
        assert_eq!(encoded["usageMetadata"]["candidatesTokenCount"], 2);
        assert_eq!(encoded["usageMetadata"]["totalTokenCount"], 3);
    }

    #[test]
    fn antigravity_stream_encoder_emits_thinking_signature() {
        let mut encoder = AntigravityStreamEncoder::new();

        let frames = encoder
            .encode_event(&NeutralStreamEvent::ThinkingSignature {
                choice_index: 2,
                signature: "signed-thinking".to_string(),
            })
            .unwrap();

        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("\"thoughtSignature\":\"signed-thinking\""));
    }

    #[test]
    fn antigravity_stream_encoder_attaches_final_usage_to_finish_frame() {
        let mut encoder = AntigravityStreamEncoder::new();

        assert!(encoder
            .encode_event(&NeutralStreamEvent::Finish {
                choice_index: 2,
                reason: FinishReason::Stop,
                raw_finish_reason: Some("stop".to_string()),
            })
            .unwrap()
            .is_empty());

        let frames = encoder
            .encode_event(&NeutralStreamEvent::ResponseEnd {
                usage: Some(UsageInfo {
                    input_tokens: 7,
                    output_tokens: 4,
                    cache_read_tokens: Some(3),
                    cache_write_tokens: Some(2),
                    reasoning_tokens: Some(5),
                    total_tokens: 21,
                }),
            })
            .unwrap();

        assert_eq!(frames.len(), 2);
        let payload: serde_json::Value =
            serde_json::from_str(frames[0].strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(payload["candidates"][0]["index"], 2);
        assert_eq!(payload["candidates"][0]["finishReason"], "STOP");
        assert_eq!(payload["candidates"][0]["content"]["parts"][0]["text"], "");
        assert_eq!(payload["usageMetadata"]["promptTokenCount"], 12);
        assert_eq!(payload["usageMetadata"]["candidatesTokenCount"], 4);
        assert_eq!(payload["usageMetadata"]["cachedContentTokenCount"], 3);
        assert_eq!(payload["usageMetadata"]["thoughtsTokenCount"], 5);
        assert_eq!(payload["usageMetadata"]["totalTokenCount"], 21);
        assert_eq!(frames[1], "data: [DONE]\n\n");
    }

    #[test]
    fn antigravity_stream_encoder_flushes_finish_when_stream_aborts() {
        let mut encoder = AntigravityStreamEncoder::new();
        encoder
            .encode_event(&NeutralStreamEvent::Finish {
                choice_index: 0,
                reason: FinishReason::Stop,
                raw_finish_reason: Some("stop".to_string()),
            })
            .unwrap();

        let frames = encoder.abort();

        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("\"finishReason\":\"STOP\""));
        assert!(!frames[0].contains("usageMetadata"));
        assert!(!frames[0].contains("[DONE]"));
        assert!(encoder.abort().is_empty());
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
                .encode_event(&NeutralStreamEvent::ResponseEnd { usage: None })
                .unwrap(),
            vec!["data: [DONE]\n\n".to_string()]
        );
        assert!(encoder
            .encode_event(&NeutralStreamEvent::ResponseEnd { usage: None })
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
