use super::*;

fn policy() -> ModelCompressionPolicy {
    ModelCompressionPolicy {
        token_threshold: 80_000,
        max_token_limit: 100_000,
        max_output_tokens: 20_000,
        ..ModelCompressionPolicy::default()
    }
}

#[test]
fn model_compression_policy_round_trips_with_complete_required_fields() {
    let policy = policy();
    let value = serde_json::to_value(&policy).unwrap();

    assert_eq!(value["checkpoint_model"], "MODEL_PLACEHOLDER_M50");
    assert_eq!(value["token_threshold"], 80_000);
    assert_eq!(value["max_token_limit"], 100_000);
    assert_eq!(value["max_output_tokens"], 20_000);
    assert_eq!(value["retry_config"]["max_retries"], 0);
    assert_eq!(
        serde_json::from_value::<ModelCompressionPolicy>(value).unwrap(),
        policy
    );
}

#[test]
fn model_compression_policy_requires_every_field_and_rejects_unknown_fields() {
    let value = serde_json::to_value(policy()).unwrap();

    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("token_threshold");
    assert!(serde_json::from_value::<ModelCompressionPolicy>(missing).is_err());

    let mut unknown = value;
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_string(), serde_json::json!(true));
    assert!(serde_json::from_value::<ModelCompressionPolicy>(unknown).is_err());
}

#[test]
fn model_compression_policy_resolves_the_same_effective_values_used_at_runtime() {
    let mut configured = policy();
    configured.token_threshold = 95_000;
    configured.max_token_limit = 120_000;
    configured.max_output_tokens = 30_000;

    let resolved = configured
        .resolve_effective(Some(100_000), Some(20_000))
        .unwrap();

    assert_eq!(resolved.token_threshold, 80_000);
    assert_eq!(resolved.max_token_limit, 100_000);
    assert_eq!(resolved.max_output_tokens, 20_000);
    assert_eq!(resolved.checkpoint_model, configured.checkpoint_model);
    assert_eq!(resolved.retry_config, configured.retry_config);
}

#[test]
fn model_compression_policy_validation_accepts_complete_valid_policy() {
    assert!(policy().validate("policy").is_ok());
}

#[test]
fn model_compression_policy_validation_accepts_inherited_placeholder_workers() {
    let mut policy = policy();
    policy.checkpoint_model = "MODEL_PLACEHOLDER_M400".to_string();

    assert!(policy.validate("policy").is_ok());
}

#[test]
fn model_compression_policy_validation_rejects_invalid_worker_models() {
    for checkpoint_model in [
        "MODEL_PLACEHOLDER_M",
        "MODEL_PLACEHOLDER_M-1",
        "custom-worker",
    ] {
        let mut policy = policy();
        policy.checkpoint_model = checkpoint_model.to_string();

        assert!(policy.validate("policy").is_err());
    }
}

#[test]
fn model_compression_policy_validation_rejects_invalid_strategy_numbers() {
    for (max_overhead_ratio, moving_window_size) in [
        ("not-a-number", "1"),
        ("-0.1", "1"),
        ("0.3", "NaN"),
        ("0.3", "-1"),
    ] {
        let mut policy = policy();
        policy.max_overhead_ratio = max_overhead_ratio.to_string();
        policy.moving_window_size = moving_window_size.to_string();

        assert!(policy.validate("policy").is_err());
    }
}

#[test]
fn model_compression_policy_validation_rejects_invalid_token_limits() {
    for (threshold, limit, output) in [
        (0, 100, 10),
        (80, 0, 10),
        (80, 100, 0),
        (100, 100, 1),
        (1, 100, 100),
        (80, 100, 21),
    ] {
        let mut policy = policy();
        policy.token_threshold = threshold;
        policy.max_token_limit = limit;
        policy.max_output_tokens = output;

        assert!(policy.validate("policy").is_err());
    }
}
