use super::custom::{
    DEFAULT_CONTEXT_WINDOW, DEFAULT_INPUT_TOKEN_LIMIT, DEFAULT_OUTPUT_TOKEN_LIMIT,
};
use super::*;
use crate::domain::{
    CheckpointLimitMode, CompressionLimitsPolicy, ModelCapabilities, ModelCheckpointOverride,
    ModelTokenLimits, OfficialModelSettings, ParameterOverrides, UpstreamModel, VirtualModel,
};
use serde_json::{json, Value};

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
            upstream_model_id: "upstream-model".to_string(),
            display_name: "Custom Model".to_string(),
            capabilities: ModelCapabilities::default(),
            token_limits: ModelTokenLimits::default(),
            checkpoint_override: None,
            tokenizer: None,
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        },
    )
}

fn checkpoint(descriptor: &Value) -> Value {
    let raw = descriptor["modelExperiments"]["experiments"]["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]
        ["stringValue"]
        .as_str()
        .expect("model must contain checkpoint settings");
    serde_json::from_str(raw).expect("checkpoint settings must be valid JSON")
}

fn absolute_policy(
    enabled: bool,
    threshold: u32,
    hard_limit: u32,
    output: u32,
) -> CompressionLimitsPolicy {
    CompressionLimitsPolicy {
        enabled,
        mode: CheckpointLimitMode::Absolute,
        token_threshold_percent: 61,
        max_token_limit_percent: 73,
        max_output_tokens_percent: 2,
        token_threshold: threshold,
        max_token_limit: hard_limit,
        max_output_tokens: output,
    }
}

#[test]
fn custom_object_and_array_catalogs_share_one_checkpoint_payload() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(372_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(128_000),
        ..ModelTokenLimits::default()
    };
    let virtual_models = [virtual_model];
    let upstream_models = [upstream_model];

    let mut object_catalog = json!({ "models": {} });
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut object_catalog,
        &virtual_models,
        &upstream_models,
        &OfficialModelSettings::default(),
    );
    let object_checkpoint = checkpoint(&object_catalog["models"]["custom-model"]);

    let mut array_catalog = json!({ "models": [] });
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut array_catalog,
        &virtual_models,
        &upstream_models,
        &OfficialModelSettings::default(),
    );
    let array_checkpoint = checkpoint(&array_catalog["models"][0]);

    assert_eq!(object_checkpoint, array_checkpoint);
    assert_eq!(
        object_checkpoint["checkpoint_model"],
        "MODEL_PLACEHOLDER_M71"
    );
    assert_eq!(object_checkpoint["token_threshold"], "226920");
    assert_eq!(object_checkpoint["max_token_limit"], "271560");
    assert_eq!(object_checkpoint["max_output_tokens"], "7440");
    assert_eq!(object_checkpoint["enabled"], true);
    assert_eq!(object_checkpoint["retry_config"]["max_retries"], 0);
}

#[test]
fn custom_model_checkpoint_policy_must_remain_enabled() {
    let mut settings = OfficialModelSettings::default();
    settings.custom_model_checkpoint.enabled = false;
    assert!(settings.validate().is_err());

    let mut settings = OfficialModelSettings::default();
    settings.custom_model.enabled = false;
    assert!(settings.validate().is_err());
}

#[test]
fn model_execution_policy_wins_global_policy_by_upstream_model_id() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.upstream_model_id = "provider-model".to_string();
    let mut settings = OfficialModelSettings::default();
    settings.custom_model_checkpoint.checkpoint_model = "MODEL_PLACEHOLDER_M50".to_string();
    let mut model_policy = settings.custom_model_checkpoint.clone();
    model_policy.checkpoint_model = "MODEL_PLACEHOLDER_M72".to_string();
    settings
        .model_checkpoint_policies
        .insert("provider-model".to_string(), model_policy);

    let mut catalog = json!({ "models": {} });
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    assert_eq!(
        checkpoint(&catalog["models"]["custom-model"])["checkpoint_model"],
        "MODEL_PLACEHOLDER_M72"
    );
}

#[test]
fn model_limit_override_wins_global_limits_and_is_clipped() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(200_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(10_000),
        ..ModelTokenLimits::default()
    };
    upstream_model.checkpoint_override = Some(ModelCheckpointOverride::Custom {
        token_threshold: 250_000,
        max_token_limit: 300_000,
        max_output_tokens: 20_000,
    });
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &OfficialModelSettings::default(),
    );

    let value = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(value["token_threshold"], "190000");
    assert_eq!(value["max_token_limit"], "200000");
    assert_eq!(value["max_output_tokens"], "10000");
}

#[test]
fn custom_absolute_global_limits_are_serialized() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(200_000),
        input_token_limit: Some(200_000),
        output_token_limit: Some(32_000),
        ..ModelTokenLimits::default()
    };
    let settings = OfficialModelSettings {
        custom_model: absolute_policy(true, 100_000, 150_000, 12_000),
        ..OfficialModelSettings::default()
    };
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    let value = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(value["token_threshold"], "100000");
    assert_eq!(value["max_token_limit"], "150000");
    assert_eq!(value["max_output_tokens"], "12000");
}

#[test]
fn official_object_and_array_catalogs_apply_new_absolute_policy() {
    let settings = OfficialModelSettings {
        gemini: absolute_policy(true, 430_000, 512_000, 16_384),
        claude: absolute_policy(true, 80_000, 100_000, 8_000),
        ..OfficialModelSettings::default()
    };
    let mut object_catalog = json!({
        "models": {
            "gemini-pro": {
                "model": "MODEL_GEMINI_PRO",
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": r#"{"enabled":false,"stale_field":true}"#
                        }
                    }
                }
            },
            "claude-sonnet": {
                "model": "MODEL_CLAUDE_SONNET",
                "maxTokens": 200_000,
                "contextWindow": 200_000,
                "maxOutputTokens": 32_000
            }
        }
    });
    AntigravityModelDescriptor::apply_official_model_overrides(&mut object_catalog, &settings);
    let gemini_checkpoint = checkpoint(&object_catalog["models"]["gemini-pro"]);
    assert_eq!(gemini_checkpoint["max_token_limit"], "512000");
    assert_eq!(gemini_checkpoint["enabled"], true);
    assert!(gemini_checkpoint.get("stale_field").is_none());
    assert_eq!(
        checkpoint(&object_catalog["models"]["claude-sonnet"])["max_token_limit"],
        "100000"
    );

    let mut array_catalog = json!([
        { "model": "MODEL_GEMINI_PRO" },
        {
            "model": "MODEL_CLAUDE_SONNET",
            "maxTokens": 200_000,
            "contextWindow": 200_000,
            "maxOutputTokens": 32_000
        }
    ]);
    AntigravityModelDescriptor::apply_official_model_overrides(&mut array_catalog, &settings);
    assert_eq!(checkpoint(&array_catalog[0])["max_token_limit"], "512000");
    assert_eq!(checkpoint(&array_catalog[1])["max_token_limit"], "100000");
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
}
