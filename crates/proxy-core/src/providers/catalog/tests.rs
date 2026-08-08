use super::parser::{parse_catalog_models, parse_catalog_models_with_context};
use super::*;
use crate::domain::{ParameterOverrides, ProviderProtocol};
use crate::tests::mock_provider::MockProviderServer;
use serde_json::json;
use std::collections::HashMap;

fn catalog_provider(models_endpoint: String) -> Provider {
    Provider {
        id: "provider-catalog".to_string(),
        name: "Catalog Provider".to_string(),
        protocol: ProviderProtocol::OpenaiChatCompletions,
        models_endpoint,
        generate_endpoint: "http://127.0.0.1:50998/v1/chat/completions".to_string(),
        api_key: "sk-catalog".to_string(),
        headers: HashMap::new(),
        default_parameters: ParameterOverrides::default(),
        connect_timeout_ms: 3000,
        request_timeout_ms: 5000,
        stream_idle_timeout_ms: 5000,
        enabled: true,
    }
}

#[test]
fn adds_cpa_catalog_version_only_for_cpa_endpoint() {
    let provider = catalog_provider("http://127.0.0.1:8317/v1/models?tenant=test".to_string());
    assert_eq!(
        catalog_models_url(&provider).unwrap().as_str(),
        "http://127.0.0.1:8317/v1/models?tenant=test&client_version=1"
    );

    let provider =
        catalog_provider("http://127.0.0.1:8317/v1/models?client_version=custom".to_string());
    assert_eq!(
        catalog_models_url(&provider).unwrap().as_str(),
        "http://127.0.0.1:8317/v1/models?client_version=custom"
    );

    let provider = catalog_provider("https://api.openai.com/v1/models".to_string());
    assert_eq!(
        catalog_models_url(&provider).unwrap().as_str(),
        "https://api.openai.com/v1/models"
    );

    let provider = catalog_provider("http://[::1]:8317/v1/models".to_string());
    assert_eq!(
        catalog_models_url(&provider).unwrap().as_str(),
        "http://[::1]:8317/v1/models?client_version=1"
    );
}

#[test]
fn parses_common_openai_and_gemini_catalog_shapes() {
    let openai = parse_catalog_models(
        &json!({
            "data": [
                {"id": "gpt-5"},
                {"id": "gpt-5"},
                {"id": "gpt-4.1", "display_name": "GPT 4.1"}
            ]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(
        openai,
        vec![
            ProviderCatalogModel {
                id: "gpt-5".to_string(),
                display_name: "gpt-5".to_string(),
                context_window: None,
                input_token_limit: None,
                output_token_limit: None,
                reasoning: None,
                ..ProviderCatalogModel::default()
            },
            ProviderCatalogModel {
                id: "gpt-4.1".to_string(),
                display_name: "GPT 4.1".to_string(),
                context_window: None,
                input_token_limit: None,
                output_token_limit: None,
                reasoning: None,
                ..ProviderCatalogModel::default()
            },
        ]
    );

    let gemini = parse_catalog_models(
        &json!({
            "models": [
                {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro"}
            ]
        }),
        &ProviderProtocol::GeminiGenerateContent,
    );
    assert_eq!(
        gemini,
        vec![ProviderCatalogModel {
            id: "gemini-2.5-pro".to_string(),
            display_name: "Gemini 2.5 Pro".to_string(),
            context_window: None,
            input_token_limit: None,
            output_token_limit: None,
            reasoning: None,
            ..ProviderCatalogModel::default()
        }]
    );
}

#[test]
fn parses_cpa_catalog_models_identified_by_slug() {
    let models = parse_catalog_models_with_context(
        &json!({
            "models": [{
                "slug": "gpt-5.6-sol",
                "display_name": "GPT 5.6 Sol",
                "context_window": 372_000,
                "max_tokens": 128_000,
                "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}]
            }]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
        true,
    );

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gpt-5.6-sol");
    assert_eq!(models[0].display_name, "GPT 5.6 Sol");
    assert_eq!(models[0].context_window, Some(372_000));
    assert_eq!(models[0].max_context_window, None);
    assert_eq!(models[0].input_token_limit, Some(372_000));
    assert_eq!(models[0].output_token_limit, Some(128_000));
    assert_eq!(
        models[0]
            .reasoning
            .as_ref()
            .map(|reasoning| reasoning.levels.clone()),
        Some(vec![ReasoningLevel::Low, ReasoningLevel::High])
    );
}

#[test]
fn normalizes_cpa_capabilities_without_dropping_raw_metadata() {
    let models = parse_catalog_models_with_context(
        &json!({
            "models": [{
                "slug": "gpt-5.6-sol",
                "input_modalities": ["text", "IMAGE"],
                "supports_parallel_tool_calls": true,
                "capabilities": {
                    "reasoning": true,
                    "vendor_extension": {"mode": "native"}
                }
            }]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
        true,
    );

    assert_eq!(
        models[0].capabilities,
        Some(json!({
            "reasoning": true,
            "vendor_extension": {"mode": "native"},
            "input_modalities": ["text", "IMAGE"],
            "supports_parallel_tool_calls": true,
            "vision": true,
            "tools": true
        }))
    );
}

#[test]
fn preserves_explicit_capability_flags_and_parallel_false_is_not_tool_unsupported() {
    let models = parse_catalog_models_with_context(
        &json!({
            "data": [
                {
                    "id": "explicit-flags",
                    "capabilities": {
                        "vision": false,
                        "tools": false,
                        "input_modalities": ["image"],
                        "supports_parallel_tool_calls": true
                    }
                },
                {
                    "id": "serial-tools-unknown",
                    "supports_parallel_tool_calls": false
                },
                {
                    "id": "generation-method-tools",
                    "experimental_supported_tools": [],
                    "supported_generation_methods": ["toolCall"]
                }
            ]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
        true,
    );

    assert_eq!(models[0].capabilities.as_ref().unwrap()["vision"], false);
    assert_eq!(models[0].capabilities.as_ref().unwrap()["tools"], false);
    assert_eq!(
        models[1].capabilities,
        Some(json!({"supports_parallel_tool_calls": false}))
    );
    assert_eq!(models[2].capabilities.as_ref().unwrap()["tools"], true);
}

#[test]
fn parses_model_metadata_maps_using_the_object_key_as_model_id() {
    let models = parse_catalog_models_with_context(
        &json!({
            "models": {
                "gpt-5.6-sol": {
                    "display_name": "GPT 5.6 Sol",
                    "context_window": 372_000,
                    "max_tokens": 128_000,
                    "reasoning": ["low", "high"]
                }
            }
        }),
        &ProviderProtocol::OpenaiChatCompletions,
        true,
    );

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gpt-5.6-sol");
    assert_eq!(models[0].input_token_limit, Some(372_000));
    assert_eq!(models[0].output_token_limit, Some(128_000));
}

#[test]
fn parses_model_specific_token_limits_and_context_window() {
    let gemini = parse_catalog_models(
        &json!({
            "models": [{
                "name": "models/gemini-2.5-pro",
                "inputTokenLimit": 1_000_000,
                "outputTokenLimit": 65_536
            }]
        }),
        &ProviderProtocol::GeminiGenerateContent,
    );
    assert_eq!(gemini[0].input_token_limit, Some(1_000_000));
    assert_eq!(gemini[0].output_token_limit, Some(65_536));

    let claude = parse_catalog_models(
        &json!({
            "data": [{
                "id": "claude-sonnet",
                "max_input_tokens": "200000",
                "max_tokens": 32_000,
                "context_length": 200_000
            }]
        }),
        &ProviderProtocol::AnthropicMessages,
    );
    assert_eq!(claude[0].input_token_limit, Some(200_000));
    assert_eq!(claude[0].output_token_limit, Some(32_000));

    let ambiguous = parse_catalog_models(
        &json!({
            "data": [{"id": "unknown", "context_length": 1_000_000}]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(ambiguous[0].context_window, None);
    assert_eq!(ambiguous[0].context_length, Some(1_000_000));
    assert_eq!(ambiguous[0].input_token_limit, None);
    assert_eq!(ambiguous[0].output_token_limit, None);

    let max_context = parse_catalog_models(
        &json!({
            "data": [{"id": "max-context", "max_context_window": 131_072}]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(max_context[0].context_window, Some(131_072));
    assert_eq!(max_context[0].max_context_window, Some(131_072));
}

#[test]
fn preserves_complete_catalog_metadata_and_uses_cpa_context_as_input() {
    let cpa = parse_catalog_models_with_context(
        &json!({
            "data": [{
                "id": "claude-sonnet",
                "context_length": 1_000_000,
                "max_tokens": 128_000,
                "token_budget": 65_536,
                "thinking": {"supported": true},
                "capabilities": {"tools": true, "reasoning": true}
            }]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
        true,
    );
    assert_eq!(cpa[0].context_length, Some(1_000_000));
    assert_eq!(cpa[0].input_token_limit, Some(1_000_000));
    assert_eq!(cpa[0].output_token_limit, Some(128_000));
    assert_eq!(cpa[0].max_tokens, Some(128_000));
    assert_eq!(cpa[0].token_budget, Some(65_536));
    assert_eq!(cpa[0].thinking, Some(json!({"supported": true})));
    assert_eq!(
        cpa[0].capabilities,
        Some(json!({"tools": true, "reasoning": true}))
    );

    let openai = parse_catalog_models(
        &json!({"data": [{"id": "plain", "max_tokens": 8_192}]}),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(openai[0].max_tokens, Some(8_192));
    assert_eq!(openai[0].output_token_limit, None);

    let mistral = parse_catalog_models(
        &json!({"data": [{"id": "mistral-large", "max_context_length": 131_072}]}),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(mistral[0].context_length, Some(131_072));
    assert_eq!(mistral[0].input_token_limit, None);
}

#[test]
fn parses_vendor_token_and_reasoning_metadata() {
    let anthropic = parse_catalog_models(
        &json!({
            "data": [{
                "id": "claude-sonnet",
                "max_input_tokens": 200_000,
                "max_tokens": 8_192,
                "capabilities": {
                    "thinking": {"supported": true},
                    "effort": {"supported_efforts": ["low", "high"]}
                }
            }]
        }),
        &ProviderProtocol::AnthropicMessages,
    );
    assert_eq!(anthropic[0].input_token_limit, Some(200_000));
    assert_eq!(anthropic[0].output_token_limit, Some(8_192));
    assert_eq!(
        anthropic[0].reasoning.as_ref().unwrap().mappings,
        BTreeMap::from([
            (
                ReasoningLevel::Low,
                ReasoningMapping::Effort("low".to_string())
            ),
            (
                ReasoningLevel::High,
                ReasoningMapping::Effort("high".to_string())
            ),
        ])
    );

    let gemini = parse_catalog_models(
        &json!({
            "models": [{
                "name": "models/gemini-2.5-pro",
                "inputTokenLimit": 1_000_000,
                "outputTokenLimit": 65_536,
                "thinking": true
            }]
        }),
        &ProviderProtocol::GeminiGenerateContent,
    );
    assert_eq!(
        gemini[0].reasoning.as_ref().unwrap().mappings,
        BTreeMap::new()
    );

    let openrouter = parse_catalog_models(
        &json!({
            "data": [{
                "id": "router-model",
                "context_length": 131_072,
                "top_provider": {
                    "context_length": 114_688,
                    "max_completion_tokens": 4_096
                },
                "reasoning": {
                    "supported_efforts": ["minimal", "high"]
                }
            }]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(openrouter[0].context_window, None);
    assert_eq!(openrouter[0].context_length, Some(131_072));
    assert_eq!(openrouter[0].input_token_limit, None);
    assert_eq!(openrouter[0].output_token_limit, Some(4_096));
    assert_eq!(
        openrouter[0].reasoning.as_ref().unwrap().mappings,
        BTreeMap::from([
            (
                ReasoningLevel::Low,
                ReasoningMapping::Effort("minimal".to_string())
            ),
            (
                ReasoningLevel::High,
                ReasoningMapping::Effort("high".to_string())
            ),
        ])
    );

    let invalid = parse_catalog_models(
        &json!({
            "data": [{
                "id": "invalid",
                "max_input_tokens": 0,
                "max_output_tokens": "not-a-number",
                "max_tokens": -1
            }]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );
    assert_eq!(invalid[0].input_token_limit, None);
    assert_eq!(invalid[0].output_token_limit, None);
}

#[test]
fn parses_reasoning_metadata_without_assuming_missing_capability() {
    let models = parse_catalog_models(
        &json!({
            "data": [
                {
                    "id": "claude-opus",
                    "thinking": {"supported": true, "levels": ["low", "high", "xhigh"]}
                },
                {"id": "plain-model"},
                {"id": "no-thinking", "capabilities": {"reasoning": false}},
                {"id": "router-model", "supported_parameters": ["reasoning_effort"]},
                {"id": "modelgate-reasoning", "type": "Reasoning"}
            ]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );

    assert_eq!(
        models[0].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(true),
            levels: vec![
                ReasoningLevel::Low,
                ReasoningLevel::High,
                ReasoningLevel::XHigh
            ],
            mappings: BTreeMap::from([
                (
                    ReasoningLevel::Low,
                    ReasoningMapping::Effort("low".to_string())
                ),
                (
                    ReasoningLevel::High,
                    ReasoningMapping::Effort("high".to_string())
                ),
                (
                    ReasoningLevel::XHigh,
                    ReasoningMapping::Effort("xhigh".to_string())
                ),
            ]),
        })
    );
    assert_eq!(models[1].reasoning, None);
    assert_eq!(
        models[2].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(false),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
        })
    );
    assert_eq!(
        models[3].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(true),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
        })
    );
    assert_eq!(
        models[4].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(true),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
        })
    );
}

#[tokio::test]
async fn fetches_catalog_with_provider_authentication() {
    let response = json!({
        "data": [
            {"id": "gpt-5.6-terra"},
            {"id": "gpt-5.6-sol"}
        ]
    })
    .to_string();
    let (mock_url, _handle, recorded) = MockProviderServer::start_recording(200, &response).await;

    let models = fetch_provider_models(&catalog_provider(format!("{mock_url}/v1/models")))
        .await
        .unwrap();

    assert_eq!(models.len(), 2);
    let recorded = recorded.await.unwrap();
    assert_eq!(recorded.path_and_query, "/v1/models");
    assert_eq!(recorded.authorization.as_deref(), Some("Bearer sk-catalog"));
}
