use super::custom::{
    DEFAULT_CONTEXT_WINDOW, DEFAULT_INPUT_TOKEN_LIMIT, DEFAULT_OUTPUT_TOKEN_LIMIT,
};
use super::*;
use crate::domain::{
    ModelCapabilities, ModelCompressionPolicy, ModelTokenLimits, ParameterOverrides,
    TokenLimitSource, UpstreamModel, VirtualModel,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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
    );
    AntigravityModelDescriptor::inject_into_model_list(
        &mut array_catalog,
        &[virtual_model],
        &[upstream_model],
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
                "maxOutputTokens": 8_000
            }
        }
    });
    let mut array_catalog = json!({
        "models": [{
            "id": "official-array",
            "maxTokens": "80_000".replace('_', ""),
            "maxOutputTokens": 6_000
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
fn deprecated_official_policy_is_applied_to_both_mapped_model_entries() {
    let mut catalog = json!({
        "models": {
            "gemini-3.1-pro-high": {
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535
            },
            "gemini-pro-agent": {
                "maxTokens": 1_048_576,
                "maxOutputTokens": 65_535
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
                "maxOutputTokens": 20_000
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
    let catalog =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(&virtual_model, &upstream_model);

    assert_eq!(descriptor["contextWindow"], DEFAULT_CONTEXT_WINDOW);
    assert_eq!(descriptor["inputTokenLimit"], DEFAULT_INPUT_TOKEN_LIMIT);
    assert_eq!(descriptor["outputTokenLimit"], DEFAULT_OUTPUT_TOKEN_LIMIT);
    assert_eq!(catalog["contextWindow"], DEFAULT_CONTEXT_WINDOW);
    assert_eq!(catalog["recommended"], false);
}

#[test]
fn video_capabilities_are_consistent_across_catalog_shapes() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.capabilities.supported_mime_types = vec![
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
    );
    AntigravityModelDescriptor::inject_into_model_list(
        &mut array_catalog,
        &[virtual_model],
        &[upstream_model],
    );

    let object_model = &object_catalog["models"]["custom-model"];
    let array_model = &array_catalog["models"][0];
    assert_eq!(object_model["supportsVideo"], true);
    assert_eq!(array_model["supportsVideo"], true);
    assert_eq!(object_model["supportedMimeTypes"]["video/mp4"], true);
    assert!(array_model["supportedMimeTypes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "video/mp4"));
    assert!(object_model["inputModalities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|modality| modality == "VIDEO"));
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
    let catalog =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(&virtual_model, &upstream_model);

    assert_eq!(descriptor["supportsThinking"], true);
    assert_eq!(descriptor["thinkingBudget"], 10_001);
    assert_eq!(descriptor["minThinkingBudget"], 128);
    assert_eq!(catalog["supportsThinking"], true);
    assert_eq!(catalog["thinkingBudget"], 10_001);
    assert_eq!(catalog["minThinkingBudget"], 128);
}
