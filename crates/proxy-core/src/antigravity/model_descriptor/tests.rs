use super::custom::{
    DEFAULT_CONTEXT_WINDOW, DEFAULT_INPUT_TOKEN_LIMIT, DEFAULT_OUTPUT_TOKEN_LIMIT,
};
use super::*;
use crate::domain::{
    ModelCapabilities, ModelCompressionPolicy, ModelModality, ModelRole, ModelTokenLimits,
    ParameterOverrides, Provider, ProviderProtocol, ReasoningCapability, ReasoningLevel,
    ReasoningMapping, TokenLimitSource, UpstreamModel, VirtualModel,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn provider(protocol: ProviderProtocol) -> Provider {
    Provider {
        id: "provider".to_string(),
        name: "Provider".to_string(),
        protocol,
        models_endpoint: "http://localhost/models".to_string(),
        generate_endpoint: "http://localhost/generate".to_string(),
        api_key: "test-key".to_string(),
        headers: HashMap::new(),
        default_parameters: ParameterOverrides::default(),
        connect_timeout_ms: 1_000,
        request_timeout_ms: 1_000,
        stream_idle_timeout_ms: 1_000,
        enabled: true,
    }
}

fn providers() -> Vec<Provider> {
    vec![provider(ProviderProtocol::GeminiGenerateContent)]
}

fn models() -> (VirtualModel, UpstreamModel) {
    (
        VirtualModel {
            id: "custom-model".to_string(),
            host_model_id: Some("MODEL_PLACEHOLDER_M400".to_string()),
            upstream_model_id: "upstream-model".to_string(),
            display_name: "Custom Model".to_string(),
            default_reasoning_level: None,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        },
        UpstreamModel {
            id: "upstream-model".to_string(),
            provider_id: "provider".to_string(),
            upstream_model_id: "provider-model".to_string(),
            display_name: "Custom Model".to_string(),
            capabilities: ModelCapabilities::default(),
            token_limits: ModelTokenLimits::default(),
            compression_policy: None,
            tokenizer: None,
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        },
    )
}

fn policy(threshold: u32, limit: u32, output: u32) -> ModelCompressionPolicy {
    ModelCompressionPolicy {
        checkpoint_model: "MODEL_PLACEHOLDER_M72".to_string(),
        token_threshold: threshold,
        max_token_limit: limit,
        max_output_tokens: output,
        ..ModelCompressionPolicy::default()
    }
}

fn checkpoint_experiments() -> Value {
    checkpoint_experiments_with_model("UPSTREAM_WORKER")
}

fn checkpoint_experiments_with_model(checkpoint_model: &str) -> Value {
    json!({
        "experiments": {
            "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                "stringValue": serde_json::to_string(&json!({
                    "enabled": false,
                    "checkpoint_model": checkpoint_model,
                    "strategy": "UPSTREAM_STRATEGY",
                    "retry_config": { "max_retries": 7 },
                    "token_threshold": "123",
                    "max_token_limit": "456",
                    "max_output_tokens": "78"
                })).unwrap()
            }
        }
    })
}

fn checkpoint(descriptor: &Value) -> Value {
    let raw = descriptor["modelExperiments"]["experiments"]["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]
        ["stringValue"]
        .as_str()
        .expect("model must contain checkpoint settings");
    serde_json::from_str(raw).expect("checkpoint settings must be valid JSON")
}

fn has_checkpoint(descriptor: &Value) -> bool {
    descriptor["modelExperiments"]["experiments"]["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]
        ["stringValue"]
        .is_string()
}

#[test]
fn custom_model_without_policy_does_not_inject_checkpointer() {
    let (virtual_model, upstream_model) = models();
    let mut object_catalog = json!({ "models": {} });
    let mut array_catalog = json!({ "models": [] });

    AntigravityModelDescriptor::inject_into_model_list(
        &mut object_catalog,
        std::slice::from_ref(&virtual_model),
        std::slice::from_ref(&upstream_model),
        &providers(),
    );
    AntigravityModelDescriptor::inject_into_model_list(
        &mut array_catalog,
        &[virtual_model],
        &[upstream_model],
        &providers(),
    );

    assert!(!has_checkpoint(&object_catalog["models"]["custom-model"]));
    assert!(!has_checkpoint(&array_catalog["models"][0]));
}

#[test]
fn custom_model_policy_is_applied_and_clamped_to_model_limits() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(200_000),
        input_token_limit: Some(180_000),
        output_token_limit: Some(10_000),
        ..ModelTokenLimits::default()
    };
    upstream_model.compression_policy = Some(policy(250_000, 300_000, 20_000));
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &providers(),
    );

    let checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(checkpoint["token_threshold"], "170000");
    assert_eq!(checkpoint["max_token_limit"], "180000");
    assert_eq!(checkpoint["max_output_tokens"], "10000");
    assert_eq!(checkpoint["checkpoint_model"], "MODEL_PLACEHOLDER_M72");
    assert_eq!(checkpoint["retry_config"]["max_retries"], 0);
}

#[test]
fn estimated_context_does_not_clamp_catalog_input_checkpointer_capacity() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(128_000),
        context_window_source: TokenLimitSource::Estimated,
        input_token_limit: Some(1_048_576),
        input_token_limit_source: TokenLimitSource::Catalog,
        output_token_limit: Some(65_535),
        output_token_limit_source: TokenLimitSource::Catalog,
    };
    upstream_model.compression_policy = Some(policy(524_288, 734_003, 65_535));
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &providers(),
    );

    let checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(checkpoint["token_threshold"], "524288");
    assert_eq!(checkpoint["max_token_limit"], "734003");
    assert_eq!(checkpoint["max_output_tokens"], "65535");
}

#[test]
fn official_policy_map_matches_object_key_and_array_id() {
    let policies = BTreeMap::from([
        (
            "official-object".to_string(),
            policy(80_000, 100_000, 20_000),
        ),
        ("official-array".to_string(), policy(60_000, 90_000, 10_000)),
    ]);
    let mut object_catalog = json!({
        "models": {
            "official-object": {
                "maxTokens": 90_000,
                "maxOutputTokens": 8_000,
                "modelExperiments": checkpoint_experiments()
            }
        }
    });
    let mut array_catalog = json!({
        "models": [{
            "id": "official-array",
            "maxTokens": "80_000".replace('_', ""),
            "maxOutputTokens": 6_000,
            "modelExperiments": checkpoint_experiments()
        }]
    });

    AntigravityModelDescriptor::apply_official_model_overrides(&mut object_catalog, &policies);
    AntigravityModelDescriptor::apply_official_model_overrides(&mut array_catalog, &policies);

    let object_checkpoint = checkpoint(&object_catalog["models"]["official-object"]);
    assert_eq!(object_checkpoint["token_threshold"], "80000");
    assert_eq!(object_checkpoint["max_token_limit"], "90000");
    assert_eq!(object_checkpoint["max_output_tokens"], "8000");

    let array_checkpoint = checkpoint(&array_catalog["models"][0]);
    assert_eq!(array_checkpoint["token_threshold"], "60000");
    assert_eq!(array_checkpoint["max_token_limit"], "80000");
    assert_eq!(array_checkpoint["max_output_tokens"], "6000");
}

#[test]
fn ordinary_model_fields_do_not_create_checkpoint_workers() {
    let catalog = json!({
        "models": {
            "ordinary-m71": {
                "model": "MODEL_PLACEHOLDER_M71",
                "maxOutputTokens": 1_024,
                "modelExperiments": checkpoint_experiments_with_model("MODEL_PLACEHOLDER_M50")
            },
            "worker-m50": {
                "model": "MODEL_PLACEHOLDER_M50",
                "maxOutputTokens": 65_535,
                "modelExperiments": checkpoint_experiments_with_model("MODEL_PLACEHOLDER_M50")
            }
        }
    });

    let workers = super::checkpoint::official_checkpoint_worker_limits(&catalog);

    assert_eq!(workers.get("MODEL_PLACEHOLDER_M50"), Some(&65_535));
    assert!(!workers.contains_key("MODEL_PLACEHOLDER_M71"));
}

#[test]
fn official_policy_output_is_clamped_to_checkpoint_model_limit() {
    let mut catalog = json!({
        "models": {
            "gemini-3.1-flash-lite": {
                "model": "MODEL_PLACEHOLDER_M50",
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535,
                "modelExperiments": checkpoint_experiments_with_model("MODEL_PLACEHOLDER_M50")
            },
            "gemini-3.6-flash-high": {
                "model": "MODEL_PLACEHOLDER_M71",
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_536,
                "modelExperiments": checkpoint_experiments_with_model("MODEL_PLACEHOLDER_M50")
            }
        }
    });
    let mut compression = policy(314_572, 524_288, 65_536);
    compression.checkpoint_model = "MODEL_PLACEHOLDER_M50".to_string();
    let policies = BTreeMap::from([("gemini-3.6-flash-high".to_string(), compression)]);

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &policies);

    let target = &catalog["models"]["gemini-3.6-flash-high"];
    assert_eq!(target["maxOutputTokens"], 65_536);
    let checkpoint = checkpoint(target);
    assert_eq!(checkpoint["max_output_tokens"], "65535");
}

#[test]
fn deprecated_official_policy_is_applied_to_both_mapped_model_entries() {
    let mut catalog = json!({
        "models": {
            "gemini-3.1-pro-high": {
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535,
                "modelExperiments": checkpoint_experiments()
            },
            "gemini-pro-agent": {
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535,
                "modelExperiments": checkpoint_experiments()
            }
        },
        "deprecatedModelIds": {
            "gemini-3.1-pro-high": {
                "newModelId": "gemini-pro-agent"
            }
        }
    });
    let policies = BTreeMap::from([(
        "gemini-3.1-pro-high".to_string(),
        policy(80_000, 100_000, 20_000),
    )]);

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &policies);

    let deprecated_checkpoint = checkpoint(&catalog["models"]["gemini-3.1-pro-high"]);
    let replacement_checkpoint = checkpoint(&catalog["models"]["gemini-pro-agent"]);
    assert_eq!(deprecated_checkpoint["token_threshold"], "80000");
    assert_eq!(deprecated_checkpoint["max_token_limit"], "100000");
    assert_eq!(deprecated_checkpoint["max_output_tokens"], "20000");
    assert_eq!(replacement_checkpoint["token_threshold"], "80000");
    assert_eq!(replacement_checkpoint["max_token_limit"], "100000");
    assert_eq!(replacement_checkpoint["max_output_tokens"], "20000");

    let mut replacement_only_catalog = catalog.clone();
    let replacement_only_policy = BTreeMap::from([(
        "gemini-pro-agent".to_string(),
        policy(80_000, 110_000, 24_000),
    )]);
    AntigravityModelDescriptor::apply_official_model_overrides(
        &mut replacement_only_catalog,
        &replacement_only_policy,
    );
    let deprecated_checkpoint =
        checkpoint(&replacement_only_catalog["models"]["gemini-3.1-pro-high"]);
    assert_eq!(deprecated_checkpoint["token_threshold"], "80000");
}

#[test]
fn official_policy_only_replaces_three_token_fields() {
    let mut catalog = json!({
        "response": {
            "models": {
                "official-default": {
                    "maxTokens": 128_000,
                    "maxOutputTokens": 16_384,
                    "modelExperiments": checkpoint_experiments()
                }
            }
        }
    });
    let original = checkpoint(&catalog["response"]["models"]["official-default"]);
    let policies = BTreeMap::from([(
        "official-default".to_string(),
        policy(50_000, 100_000, 10_000),
    )]);

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &policies);

    let modified = checkpoint(&catalog["response"]["models"]["official-default"]);
    assert_eq!(modified["token_threshold"], "50000");
    assert_eq!(modified["max_token_limit"], "100000");
    assert_eq!(modified["max_output_tokens"], "10000");
    for field in ["enabled", "checkpoint_model", "strategy", "retry_config"] {
        assert_eq!(modified[field], original[field]);
    }
}

#[test]
fn model_injection_supports_root_and_response_object_and_array_catalogs() {
    let (virtual_model, upstream_model) = models();
    let sorts = || {
        json!([{
            "groups": [{ "modelIds": ["official"] }]
        }])
    };
    let mut catalogs = [
        json!({ "models": {}, "agentModelSorts": sorts() }),
        json!({ "response": { "models": {}, "agentModelSorts": sorts() } }),
        json!({ "models": [], "agentModelSorts": sorts() }),
        json!({ "response": { "models": [], "agentModelSorts": sorts() } }),
    ];

    for catalog in &mut catalogs {
        AntigravityModelDescriptor::inject_into_model_list(
            catalog,
            std::slice::from_ref(&virtual_model),
            std::slice::from_ref(&upstream_model),
            &providers(),
        );
    }

    assert!(catalogs[0]["models"]["custom-model"].is_object());
    assert!(catalogs[1]["response"]["models"]["custom-model"].is_object());
    assert_eq!(catalogs[2]["models"][0]["id"], "custom-model");
    assert_eq!(catalogs[3]["response"]["models"][0]["id"], "custom-model");
    for catalog in [&catalogs[0], &catalogs[2]] {
        assert_eq!(
            catalog["agentModelSorts"][0]["groups"][0]["modelIds"],
            json!(["official", "custom-model"])
        );
    }
    for catalog in &catalogs {
        assert_eq!(AntigravityModelDescriptor::model_count(catalog), 1);
    }
}

#[test]
fn non_ascii_reasoning_model_names_are_safe_in_tiered_catalog() {
    let (mut virtual_model, mut upstream_model) = models();
    virtual_model.display_name = "中文模型".to_string();
    upstream_model.capabilities.reasoning = ReasoningCapability {
        levels: BTreeMap::from([(
            ReasoningLevel::High,
            ReasoningMapping::NativeLevel("high".to_string()),
        )]),
        ..ReasoningCapability::default()
    };
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list(
        &mut catalog,
        std::slice::from_ref(&virtual_model),
        std::slice::from_ref(&upstream_model),
        &providers(),
    );

    assert_eq!(
        catalog["models"]["custom-model-tiered"]["displayName"],
        "中文模型"
    );
}

#[test]
fn image_generation_model_is_registered_without_entering_agent_sort() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.capabilities.roles = BTreeSet::from([ModelRole::ImageGeneration]);
    upstream_model.capabilities.output_modalities = BTreeSet::from([ModelModality::Image]);
    let mut catalog = json!({
        "models": {},
        "agentModelSorts": [{
            "displayName": "Recommended",
            "groups": [{ "modelIds": ["official-agent", "custom-model"] }]
        }],
        "imageGenerationModelIds": ["official-image", "custom-model"]
    });

    for _ in 0..2 {
        AntigravityModelDescriptor::inject_into_model_list(
            &mut catalog,
            std::slice::from_ref(&virtual_model),
            std::slice::from_ref(&upstream_model),
            &providers(),
        );
    }

    assert!(catalog["models"]["custom-model"].is_object());
    assert_eq!(
        catalog["imageGenerationModelIds"],
        json!(["official-image", "custom-model"])
    );
    assert_eq!(
        catalog["agentModelSorts"][0]["groups"][0]["modelIds"],
        json!(["official-agent"])
    );
    assert!(catalog["agentModelSorts"]
        .as_array()
        .is_some_and(|sorts| sorts.iter().all(|sort| sort["displayName"] != "BYOK")));
}

#[test]
fn disabled_official_models_are_removed_from_models_and_agent_sorts() {
    let mut catalog = json!({
        "models": {
            "gemini-2.5-flash": { "displayName": "Gemini 2.5 Flash" },
            "claude-3-5-sonnet": { "displayName": "Claude 3.5 Sonnet" }
        },
        "agentModelSorts": [{
            "groups": [{ "modelIds": ["gemini-2.5-flash", "claude-3-5-sonnet"] }]
        }]
    });
    let mut disabled = std::collections::HashSet::new();
    disabled.insert("gemini-2.5-flash".to_string());

    AntigravityModelDescriptor::remove_disabled_official_models(&mut catalog, &disabled);

    assert!(catalog["models"].get("gemini-2.5-flash").is_none());
    assert!(catalog["models"].get("claude-3-5-sonnet").is_some());
    let remaining_sorts = &catalog["agentModelSorts"][0]["groups"][0]["modelIds"];
    assert_eq!(remaining_sorts, &json!(["claude-3-5-sonnet"]));
}

#[test]
fn official_model_without_policy_keeps_upstream_checkpointer_unchanged() {
    let original = r#"{"enabled":false,"token_threshold":"123"}"#;
    let mut catalog = json!({
        "models": {
            "official-default": {
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": original
                        }
                    }
                }
            }
        }
    });

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &BTreeMap::new());

    assert_eq!(
        catalog["models"]["official-default"]["modelExperiments"]["experiments"]
            ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"],
        original
    );
}

#[test]
fn official_placeholder_m400_entry_is_not_skipped() {
    let mut catalog = json!({
        "models": {
            "official-placeholder": {
                "id": "official-placeholder",
                "model": "MODEL_PLACEHOLDER_M400",
                "maxTokens": 100_000,
                "maxOutputTokens": 20_000,
                "modelExperiments": checkpoint_experiments()
            }
        }
    });
    let policies = BTreeMap::from([(
        "official-placeholder".to_string(),
        policy(70_000, 90_000, 10_000),
    )]);

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &policies);

    assert_eq!(
        checkpoint(&catalog["models"]["official-placeholder"])["max_token_limit"],
        "90000"
    );
}

#[test]
fn descriptor_uses_experience_defaults_for_missing_model_limits() {
    let (virtual_model, upstream_model) = models();
    let descriptor =
        AntigravityModelDescriptor::build_model_object(&virtual_model, &upstream_model);
    let catalog = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
        &virtual_model,
        &upstream_model,
        &provider(ProviderProtocol::GeminiGenerateContent),
    );

    assert_eq!(descriptor["contextWindow"], DEFAULT_CONTEXT_WINDOW);
    assert_eq!(descriptor["inputTokenLimit"], DEFAULT_INPUT_TOKEN_LIMIT);
    assert_eq!(descriptor["outputTokenLimit"], DEFAULT_OUTPUT_TOKEN_LIMIT);
    assert_eq!(catalog["maxTokens"], DEFAULT_INPUT_TOKEN_LIMIT);
    assert_eq!(catalog["recommended"], false);
    assert_eq!(
        catalog
            .as_object()
            .expect("catalog entry must be an object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "apiProvider",
            "displayName",
            "maxOutputTokens",
            "maxTokens",
            "model",
            "modelProvider",
            "planModel",
            "recommended",
            "requestedModel",
            "supportedMimeTypes",
            "supportsImages",
            "supportsThinking",
            "supportsVideo",
            "tagDescription",
            "tagTitle",
        ])
    );
}

#[test]
fn cloud_code_catalog_keeps_byok_transport_and_maps_model_provider_by_protocol() {
    let (virtual_model, upstream_model) = models();
    for (protocol, expected_model_provider) in [
        (
            ProviderProtocol::OpenaiChatCompletions,
            "MODEL_PROVIDER_OPENAI",
        ),
        (ProviderProtocol::OpenaiResponses, "MODEL_PROVIDER_OPENAI"),
        (
            ProviderProtocol::AnthropicMessages,
            "MODEL_PROVIDER_ANTHROPIC",
        ),
        (
            ProviderProtocol::GeminiGenerateContent,
            "MODEL_PROVIDER_GOOGLE",
        ),
    ] {
        let catalog = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
            &virtual_model,
            &upstream_model,
            &provider(protocol),
        );

        assert_eq!(catalog["apiProvider"], "API_PROVIDER_GOOGLE_GEMINI");
        assert_eq!(catalog["modelProvider"], expected_model_provider);
    }
}

#[test]
fn video_capabilities_follow_each_catalog_shape() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.capabilities.input_modalities = std::collections::BTreeSet::from([
        ModelModality::Text,
        ModelModality::Image,
        ModelModality::Video,
    ]);
    upstream_model.capabilities.input_mime_types = vec![
        "image/png".to_string(),
        "video/mp4".to_string(),
        "video/webm".to_string(),
    ];
    let mut object_catalog = json!({ "models": {} });
    let mut array_catalog = json!({ "models": [] });

    AntigravityModelDescriptor::inject_into_model_list(
        &mut object_catalog,
        std::slice::from_ref(&virtual_model),
        std::slice::from_ref(&upstream_model),
        &providers(),
    );
    AntigravityModelDescriptor::inject_into_model_list(
        &mut array_catalog,
        &[virtual_model],
        &[upstream_model],
        &providers(),
    );

    let object_model = &object_catalog["models"]["custom-model"];
    let array_model = &array_catalog["models"][0];
    assert_eq!(object_model["supportsVideo"], true);
    assert_eq!(array_model["supportsVideo"], true);
    assert!(object_model.get("outputModalities").is_none());
    assert_eq!(array_model["outputModalities"], json!(["TEXT"]));
    assert_eq!(object_model["supportedMimeTypes"]["video/mp4"], true);
    assert!(array_model["supportedMimeTypes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "video/mp4"));
    assert!(object_model.get("inputModalities").is_none());
    assert!(array_model["inputModalities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|modality| modality == "VIDEO"));
}

#[test]
fn model_thinking_budgets_are_preserved_across_catalog_shapes() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.capabilities.reasoning.thinking_budget = Some(10_001);
    upstream_model.capabilities.reasoning.min_thinking_budget = Some(128);

    let descriptor =
        AntigravityModelDescriptor::build_model_object(&virtual_model, &upstream_model);
    let catalog = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
        &virtual_model,
        &upstream_model,
        &provider(ProviderProtocol::GeminiGenerateContent),
    );

    assert_eq!(descriptor["supportsThinking"], true);
    assert_eq!(descriptor["thinkingBudget"], 10_001);
    assert_eq!(descriptor["minThinkingBudget"], 128);
    assert_eq!(catalog["supportsThinking"], true);
    assert_eq!(catalog["thinkingBudget"], 10_001);
    assert_eq!(catalog["minThinkingBudget"], 128);
}
