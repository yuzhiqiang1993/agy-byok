#[cfg(test)]
mod tests {
    use crate::domain::*;
    use crate::proxy::ProxyServer;
    use crate::storage::{AppConfig, ConfigStore, KeyStore, MemoryKeyStore};
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_proxy_server_end_to_end_chat() {
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
            reasoning_variant: None,
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

    #[test]
    fn test_model_list_injection() {
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
                    thinking: true,
                },
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            virtual_models: vec![VirtualModel {
                id: "vm-claude".to_string(),
                upstream_model_id: "um-1".to_string(),
                display_name: "Claude 3.5 Sonnet BYOK".to_string(),
                reasoning_variant: None,
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
}
