use super::*;

fn percentage_policy(
    enabled: bool,
    token_threshold_percent: u8,
    max_token_limit_percent: u8,
    max_output_tokens_percent: u8,
) -> CompressionLimitsPolicy {
    CompressionLimitsPolicy {
        enabled,
        mode: CheckpointLimitMode::Percentage,
        token_threshold_percent,
        max_token_limit_percent,
        max_output_tokens_percent,
        token_threshold: 0,
        max_token_limit: 0,
        max_output_tokens: 0,
    }
}

fn absolute_policy(
    enabled: bool,
    token_threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) -> CompressionLimitsPolicy {
    CompressionLimitsPolicy {
        enabled,
        mode: CheckpointLimitMode::Absolute,
        token_threshold_percent: 61,
        max_token_limit_percent: 73,
        max_output_tokens_percent: 2,
        token_threshold,
        max_token_limit,
        max_output_tokens,
    }
}

#[test]
fn new_schema_defaults_enable_custom_checkpoint_with_m71() {
    let settings = OfficialModelSettings::default();

    assert!(!settings.gemini.enabled);
    assert!(!settings.claude.enabled);
    assert!(settings.custom_model.enabled);
    assert!(settings.custom_model_checkpoint.enabled);
    assert_eq!(
        settings.custom_model_checkpoint.checkpoint_model,
        "MODEL_PLACEHOLDER_M71"
    );
    assert!(settings.model_checkpoint_policies.is_empty());
}

#[test]
fn new_schema_requires_all_fields_and_rejects_unknown_fields() {
    let value = serde_json::to_value(OfficialModelSettings::default()).unwrap();

    let mut missing = value.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("custom_model_checkpoint");
    assert!(serde_json::from_value::<OfficialModelSettings>(missing).is_err());

    let mut unknown = value;
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unexpected_field".to_string(), serde_json::json!("value"));
    assert!(serde_json::from_value::<OfficialModelSettings>(unknown).is_err());
}

#[test]
fn execution_policy_defaults_round_trip_without_implicit_defaults() {
    let policy = CheckpointExecutionPolicy::default();
    let value = serde_json::to_value(&policy).unwrap();

    assert_eq!(value["checkpoint_model"], "MODEL_PLACEHOLDER_M71");
    assert_eq!(value["retry_config"]["max_retries"], 0);
    assert_eq!(
        serde_json::from_value::<CheckpointExecutionPolicy>(value).unwrap(),
        policy
    );
}

#[test]
fn compression_limit_validation_enforces_mode_specific_fields() {
    assert!(percentage_policy(true, 61, 73, 2)
        .validate("custom")
        .is_ok());
    assert!(absolute_policy(true, 100, 200, 20)
        .validate("custom")
        .is_ok());
    assert!(percentage_policy(true, 73, 73, 2)
        .validate("custom")
        .is_err());
    assert!(absolute_policy(true, 200, 200, 20)
        .validate("custom")
        .is_err());
    assert!(absolute_policy(true, 100, 200, 110)
        .validate("custom")
        .is_err());
}

#[test]
fn custom_percentage_limits_scale_and_clip_to_model_capabilities() {
    let settings = OfficialModelSettings {
        custom_model: percentage_policy(true, 60, 80, 5),
        ..OfficialModelSettings::default()
    };

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 372_000, 128_000),
        Some(EffectiveCheckpointLimits::new(223_200, 297_600, 18_600))
    );

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(
            Some(&ModelCheckpointOverride::Custom {
                token_threshold: 250_000,
                max_token_limit: 300_000,
                max_output_tokens: 20_000,
            }),
            200_000,
            10_000,
        ),
        Some(EffectiveCheckpointLimits::new(190_000, 200_000, 10_000))
    );
}

#[test]
fn tiny_valid_capacity_still_produces_checkpoint_limits() {
    let settings = OfficialModelSettings::default();

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 2, 1),
        Some(EffectiveCheckpointLimits::new(1, 2, 1))
    );
}

#[test]
fn absolute_limits_are_used_without_percentage_fallback() {
    let settings = OfficialModelSettings {
        custom_model: absolute_policy(true, 100_000, 150_000, 12_000),
        ..OfficialModelSettings::default()
    };

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 200_000, 32_000),
        Some(EffectiveCheckpointLimits::new(100_000, 150_000, 12_000))
    );
}

#[test]
fn model_checkpoint_policy_overrides_global_execution_policy() {
    let mut settings = OfficialModelSettings::default();
    settings.custom_model_checkpoint.checkpoint_model = "MODEL_PLACEHOLDER_M50".to_string();
    let mut model_policy = CheckpointExecutionPolicy::default();
    model_policy.checkpoint_model = "MODEL_PLACEHOLDER_M72".to_string();
    settings
        .model_checkpoint_policies
        .insert("provider-model".to_string(), model_policy.clone());

    assert_eq!(
        settings.custom_model_checkpoint_policy("provider-model"),
        &model_policy
    );
    assert_eq!(
        settings
            .custom_model_checkpoint_policy("other-model")
            .checkpoint_model,
        "MODEL_PLACEHOLDER_M50"
    );
}

#[test]
fn app_settings_validation_covers_execution_policies() {
    let mut settings = OfficialModelSettings::default();
    assert!(settings.validate().is_ok());

    settings.custom_model_checkpoint.checkpoint_model = "MODEL_UNSUPPORTED".to_string();
    assert!(settings.validate().is_err());

    let mut settings = OfficialModelSettings::default();
    settings
        .model_checkpoint_policies
        .insert(String::new(), CheckpointExecutionPolicy::default());
    assert!(settings.validate().is_err());
}
