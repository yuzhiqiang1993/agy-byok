use super::parser::{
    parse_catalog_models, parse_catalog_models_with_context, parse_official_catalog_models,
};
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
fn parses_macos_and_windows_language_server_commands() {
    let listing = r#"
101 /Applications/Antigravity.app/Contents/Resources/bin/language_server --subclient_type hub --https_server_port 0 --csrf_token mac-token --enable_sidecars
202 "C:\Users\Demo\AppData\Local\Programs\Antigravity\resources\bin\language_server.exe" --subclient_type=hub --https_server_port=61234 --csrf_token=windows-token
303 /Applications/Antigravity IDE.app/language_server --csrf_token ide-token --extension_server_port 60000 --subclient_type ide
"#;

    assert_eq!(
        parse_language_server_processes(listing),
        vec![
            LanguageServerProcess {
                pid: 101,
                source: OfficialCatalogSource::App,
                csrf: "mac-token".to_string(),
                configured_port: None,
            },
            LanguageServerProcess {
                pid: 202,
                source: OfficialCatalogSource::App,
                csrf: "windows-token".to_string(),
                configured_port: Some(61234),
            },
            LanguageServerProcess {
                pid: 303,
                source: OfficialCatalogSource::Ide,
                csrf: "ide-token".to_string(),
                configured_port: None,
            },
        ]
    );
}

#[test]
fn parses_and_deduplicates_listening_ports() {
    let listing = "n127.0.0.1:59240\nn[::1]:59241\n59240\ninvalid";

    assert_eq!(parse_listening_ports(listing), vec![59240, 59241]);
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
fn parses_checkpointer_payload_numbers_and_strings_without_fabricating_missing_fields() {
    let models = parse_catalog_models(
        &json!({
            "models": [
                {
                    "id": "string-policy",
                    "modelExperiments": {
                        "experiments": {
                            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                                "stringValue": r#"{"enabled":true,"token_threshold":"80000","max_token_limit":"100000","max_output_tokens":"20000","checkpoint_model":"MODEL_PLACEHOLDER_M71","use_last_planner_model":true}"#
                            }
                        }
                    }
                },
                {
                    "id": "number-policy",
                    "modelExperiments": {
                        "experiments": {
                            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                                "stringValue": r#"{"enabled":false,"token_threshold":60000,"max_token_limit":90000,"max_output_tokens":10000}"#
                            }
                        }
                    }
                },
                {
                    "id": "missing-enabled",
                    "modelExperiments": {
                        "experiments": {
                            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                                "stringValue": r#"{"token_threshold":"80000","max_token_limit":"100000"}"#
                            }
                        }
                    }
                },
                {
                    "id": "missing-limit",
                    "modelExperiments": {
                        "experiments": {
                            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                                "stringValue": r#"{"enabled":true,"token_threshold":"80000"}"#
                            }
                        }
                    }
                }
            ]
        }),
        &ProviderProtocol::OpenaiChatCompletions,
    );

    assert_eq!(
        models[0].upstream_compression,
        Some(UpstreamCompressionPolicy {
            enabled: true,
            token_threshold: 80_000,
            max_token_limit: 100_000,
            max_output_tokens: Some(20_000),
            checkpoint_model: Some("MODEL_PLACEHOLDER_M71".to_string()),
            use_last_planner_model: Some(true),
        })
    );
    assert_eq!(
        models[1].upstream_compression,
        Some(UpstreamCompressionPolicy {
            enabled: false,
            token_threshold: 60_000,
            max_token_limit: 90_000,
            max_output_tokens: Some(10_000),
            checkpoint_model: None,
            use_last_planner_model: None,
        })
    );
    assert_eq!(models[2].upstream_compression, None);
    assert_eq!(models[3].upstream_compression, None);
}

#[test]
fn parses_official_direct_catalog_token_limits_and_checkpointer_metadata() {
    let models = parse_official_catalog_models(&json!({
        "response": {
            "models": {
                "official-model": {
                    "displayName": "Official Model",
                    "maxTokens": "200000",
                    "contextWindow": 180000,
                    "inputTokenLimit": "150000",
                    "maxOutputTokens": 32000,
                    "outputTokenLimit": "16000",
                    "modelExperiments": {
                        "experiments": {
                            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                                "stringValue": r#"{"enabled":true,"token_threshold":"150000","max_token_limit":200000,"max_output_tokens":"32000","use_last_planner_model":false}"#
                            }
                        }
                    }
                }
            }
        }
    }));

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "official-model");
    assert_eq!(models[0].max_tokens, Some(200_000));
    assert_eq!(models[0].context_window, Some(180_000));
    assert_eq!(models[0].input_token_limit, Some(150_000));
    assert_eq!(models[0].output_token_limit, Some(16_000));
    assert_eq!(
        models[0].upstream_compression,
        Some(UpstreamCompressionPolicy {
            enabled: true,
            token_threshold: 150_000,
            max_token_limit: 200_000,
            max_output_tokens: Some(32_000),
            checkpoint_model: None,
            use_last_planner_model: Some(false),
        })
    );
}

#[test]
fn parses_complete_checkpointer_as_default_compression_policy() {
    let models = parse_official_catalog_models(&json!({
        "models": {
            "gemini-pro": {
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": r#"{"checkpoint_model":"MODEL_PLACEHOLDER_M71","enabled":true,"include_artifact_snapshots":true,"include_conversation_log":true,"include_last_user_message":false,"include_running_task_snapshots":true,"include_subagent_snapshots":true,"is_sync":false,"max_output_tokens":"65535","max_overhead_ratio":"0.30","max_token_limit":"734003","max_user_requests":10,"moving_window_size":"1","retry_config":{"exponential_multiplier":2,"include_error_feedback":false,"initial_sleep_duration_ms":1000,"max_retries":0},"strategy":"CHECKPOINT_STRATEGY_UNSPECIFIED","token_threshold":"524288","use_last_planner_model":true}"#
                        }
                    }
                }
            }
        }
    }));

    let policy = models[0].default_compression_policy.as_ref().unwrap();
    assert!(policy.enabled);
    assert_eq!(policy.checkpoint_model, "MODEL_PLACEHOLDER_M71");
    assert_eq!(policy.token_threshold, 524_288);
    assert_eq!(policy.max_token_limit, 734_003);
    assert_eq!(policy.max_output_tokens, 65_535);
    assert!(policy.use_last_planner_model);
    assert_eq!(policy.retry_config.initial_sleep_duration_ms, 1_000);
}

#[test]
fn uses_official_max_tokens_as_input_limit_when_no_explicit_input_limit_exists() {
    let models = parse_official_catalog_models(&json!({
        "models": {
            "official-model": {
                "maxTokens": 200000
            }
        }
    }));

    assert_eq!(models[0].max_tokens, Some(200_000));
    assert_eq!(models[0].input_token_limit, Some(200_000));
}

#[test]
fn parses_official_video_and_mime_capabilities() {
    let models = parse_official_catalog_models(&json!({
        "models": {
            "gemini-pro": {
                "displayName": "Gemini Pro",
                "supportsImages": true,
                "supportsVideo": true,
                "supportsThinking": true,
                "thinkingBudget": 10001,
                "minThinkingBudget": 128,
                "supportedMimeTypes": {
                    "image/heic": true,
                    "image/png": true,
                    "video/mp4": true,
                    "video/webm": true,
                    "video/disabled": false
                }
            }
        }
    }));

    assert_eq!(models[0].supports_images, Some(true));
    assert_eq!(models[0].supports_video, Some(true));
    assert_eq!(models[0].reasoning.as_ref().unwrap().supported, Some(true));
    assert_eq!(
        models[0].reasoning.as_ref().unwrap().thinking_budget,
        Some(10_001)
    );
    assert_eq!(
        models[0].reasoning.as_ref().unwrap().min_thinking_budget,
        Some(128)
    );
    assert_eq!(
        models[0].supported_mime_types,
        Some(vec![
            "image/heic".to_string(),
            "image/png".to_string(),
            "video/mp4".to_string(),
            "video/webm".to_string(),
        ])
    );
}

#[test]
fn gemini_catalog_uses_max_tokens_as_input_limit() {
    let models = parse_catalog_models(
        &json!({
            "models": [{
                "name": "models/gemini-pro",
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535
            }]
        }),
        &ProviderProtocol::GeminiGenerateContent,
    );

    assert_eq!(models[0].input_token_limit, Some(1_048_576));
    assert_eq!(models[0].output_token_limit, Some(65_535));
}

#[test]
fn parses_dynamic_and_disabled_gemini_thinking_budgets() {
    let models = parse_catalog_models(
        &json!({
            "models": [
                {
                    "name": "models/dynamic-thinking",
                    "supportsThinking": true,
                    "thinkingBudget": -1,
                    "minThinkingBudget": 128
                },
                {
                    "name": "models/thinking-disabled",
                    "supportsThinking": true,
                    "thinkingBudget": 0
                },
                {
                    "name": "models/explicitly-unsupported",
                    "supportsThinking": false,
                    "reasoning": { "levels": ["high"] }
                }
            ]
        }),
        &ProviderProtocol::GeminiGenerateContent,
    );

    assert_eq!(
        models[0].reasoning.as_ref().unwrap().thinking_budget,
        Some(-1)
    );
    assert_eq!(
        models[0].reasoning.as_ref().unwrap().min_thinking_budget,
        Some(128)
    );
    assert_eq!(
        models[1].reasoning.as_ref().unwrap().thinking_budget,
        Some(0)
    );
    assert_eq!(models[2].reasoning.as_ref().unwrap().supported, Some(false));
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
            thinking_budget: None,
            min_thinking_budget: None,
        })
    );
    assert_eq!(models[1].reasoning, None);
    assert_eq!(
        models[2].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(false),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
            thinking_budget: None,
            min_thinking_budget: None,
        })
    );
    assert_eq!(
        models[3].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(true),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
            thinking_budget: None,
            min_thinking_budget: None,
        })
    );
    assert_eq!(
        models[4].reasoning,
        Some(ProviderCatalogReasoning {
            supported: Some(true),
            levels: Vec::new(),
            mappings: BTreeMap::new(),
            thinking_budget: None,
            min_thinking_budget: None,
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
