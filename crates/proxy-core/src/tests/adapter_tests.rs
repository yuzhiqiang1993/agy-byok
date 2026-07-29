#[cfg(test)]
mod tests {
    use crate::domain::*;
    use crate::providers::*;
    use crate::routing::{ResolvedRoute, RouteTable};
    use serde_json::json;
    use std::collections::HashMap;

    fn create_dummy_route(protocol: ProviderProtocol) -> ResolvedRoute {
        ResolvedRoute {
            virtual_model: VirtualModel {
                id: "vm-1".to_string(),
                upstream_model_id: "um-1".to_string(),
                display_name: "Virtual Model 1".to_string(),
                reasoning_variant: Some(ReasoningVariant {
                    label: "High".to_string(),
                    request_field: "reasoning_effort".to_string(),
                    request_value: "high".to_string(),
                }),
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            },
            upstream_model: UpstreamModel {
                id: "um-1".to_string(),
                provider_id: "p-1".to_string(),
                upstream_model_id: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                capabilities: ModelCapabilities::default(),
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            },
            provider: Provider {
                id: "p-1".to_string(),
                name: "Test Provider".to_string(),
                protocol,
                models_endpoint: "http://localhost/models".to_string(),
                generate_endpoint: "http://localhost/chat/completions".to_string(),
                api_key_ref: "key-1".to_string(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 5000,
                request_timeout_ms: 10000,
                stream_idle_timeout_ms: 10000,
                enabled: true,
            },
            final_parameters: ParameterOverrides {
                temperature: Some(0.7),
                max_tokens: Some(2048),
                top_p: None,
                top_k: None,
                extra_body: None,
            },
        }
    }

    #[test]
    fn test_parameter_merge_priority() {
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
    fn test_extra_body_blacklist_sanitization() {
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
    fn test_openai_adapter_build_payload() {
        let adapter = OpenAIAdapter::new();
        let route = create_dummy_route(ProviderProtocol::Openai);
        let request = NeutralChatRequest {
            virtual_model_id: "vm-1".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Hello AI".to_string())],
            }],
            system_instruction: Some("You are a helpful assistant".to_string()),
            tools: vec![],
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let payload = adapter.build_request_payload(&route, &request).unwrap();
        assert_eq!(payload["model"], "gpt-4o");
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["content"], "Hello AI");
    }

    #[test]
    fn test_anthropic_adapter_build_payload() {
        let adapter = AnthropicAdapter::new();
        let route = create_dummy_route(ProviderProtocol::Anthropic);
        let request = NeutralChatRequest {
            virtual_model_id: "vm-1".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Hello Claude".to_string())],
            }],
            system_instruction: Some("System Prompt".to_string()),
            tools: vec![],
            stream: true,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let payload = adapter.build_request_payload(&route, &request).unwrap();
        assert_eq!(payload["system"], "System Prompt");
        assert_eq!(payload["stream"], true);
    }

    #[test]
    fn test_gemini_adapter_build_payload() {
        let adapter = GeminiAdapter::new();
        let route = create_dummy_route(ProviderProtocol::Gemini);
        let request = NeutralChatRequest {
            virtual_model_id: "vm-1".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("Hello Gemini".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let payload = adapter.build_request_payload(&route, &request).unwrap();
        assert_eq!(payload["contents"][0]["role"], "user");
        assert_eq!(payload["contents"][0]["parts"][0]["text"], "Hello Gemini");
    }
}
