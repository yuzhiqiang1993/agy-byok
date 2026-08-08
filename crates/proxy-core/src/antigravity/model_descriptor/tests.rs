use super::custom::{
    DEFAULT_CONTEXT_WINDOW, DEFAULT_INPUT_TOKEN_LIMIT, DEFAULT_OUTPUT_TOKEN_LIMIT,
};
use super::*;
use crate::domain::{
    CompressionPercentages, CustomModelCompressionProfile, CustomModelCompressionSettings,
    ModelCapabilities, ModelCheckpointOverride, ModelTokenLimits, OfficialCompressionProfile,
    OfficialCompressionSettings, OfficialModelSettings, ParameterOverrides, UpstreamModel,
    VirtualModel,
};
use serde_json::{json, Value};

fn official_compression(profile: OfficialCompressionProfile) -> OfficialCompressionSettings {
    OfficialCompressionSettings {
        profile,
        percentages: CompressionPercentages::default(),
    }
}

fn custom_compression(
    profile: CustomModelCompressionProfile,
    token_threshold: u8,
    max_token_limit: u8,
    max_output_tokens: u8,
) -> CustomModelCompressionSettings {
    CustomModelCompressionSettings {
        profile,
        percentages: CompressionPercentages {
            token_threshold,
            max_token_limit,
            max_output_tokens,
        },
    }
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
        .expect("custom model must contain checkpoint settings");
    serde_json::from_str(raw).expect("checkpoint settings must be valid JSON")
}

#[test]
fn uses_experience_defaults_when_limits_are_missing() {
    let (virtual_model, upstream_model) = models();

    let descriptor =
        AntigravityModelDescriptor::build_model_object(&virtual_model, &upstream_model);
    let catalog =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(&virtual_model, &upstream_model);

    assert_eq!(descriptor["contextWindow"], DEFAULT_CONTEXT_WINDOW);
    assert_eq!(descriptor["inputTokenLimit"], DEFAULT_INPUT_TOKEN_LIMIT);
    assert_eq!(descriptor["outputTokenLimit"], DEFAULT_OUTPUT_TOKEN_LIMIT);
    assert_eq!(catalog["contextWindow"], DEFAULT_CONTEXT_WINDOW);
    assert_eq!(catalog["maxTokens"], DEFAULT_INPUT_TOKEN_LIMIT);
    assert_eq!(catalog["maxOutputTokens"], DEFAULT_OUTPUT_TOKEN_LIMIT);
}

#[test]
fn uses_explicit_model_limits_in_both_descriptors() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(1_000_000),
        input_token_limit: Some(1_000_000),
        output_token_limit: Some(65_536),
        ..ModelTokenLimits::default()
    };

    let descriptor =
        AntigravityModelDescriptor::build_model_object(&virtual_model, &upstream_model);
    let catalog =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(&virtual_model, &upstream_model);

    assert_eq!(descriptor["contextWindow"], 1_000_000);
    assert_eq!(descriptor["inputTokenLimit"], 1_000_000);
    assert_eq!(descriptor["outputTokenLimit"], 65_536);
    assert_eq!(catalog["contextWindow"], 1_000_000);
    assert_eq!(catalog["maxTokens"], 1_000_000);
    assert_eq!(catalog["maxOutputTokens"], 65_536);
}

#[test]
fn adds_checkpoint_experiments_to_custom_catalog_entries() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(372_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(128_000),
        ..ModelTokenLimits::default()
    };
    let virtual_models = [virtual_model];
    let upstream_models = [upstream_model];
    let settings = OfficialModelSettings::default();

    let mut object_catalog = json!({ "models": {} });
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut object_catalog,
        &virtual_models,
        &upstream_models,
        &settings,
    );
    let object_checkpoint = checkpoint(&object_catalog["models"]["custom-model"]);

    let mut array_catalog = json!({ "models": [] });
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut array_catalog,
        &virtual_models,
        &upstream_models,
        &settings,
    );
    let array_checkpoint = checkpoint(&array_catalog["models"][0]);

    for checkpoint in [object_checkpoint, array_checkpoint] {
        assert_eq!(checkpoint["token_threshold"], "227050");
        assert_eq!(checkpoint["max_token_limit"], "272460");
        assert_eq!(checkpoint["max_output_tokens"], "5812");
        assert_eq!(checkpoint["checkpoint_model"], "MODEL_PLACEHOLDER_M400");
    }
}

#[test]
fn model_percentage_override_wins_global_and_is_scoped_to_upstream_model() {
    let (first_virtual_model, mut first_upstream_model) = models();
    first_upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(372_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(128_000),
        ..ModelTokenLimits::default()
    };
    first_upstream_model.checkpoint_override = Some(ModelCheckpointOverride::Percentage {
        threshold_percent: 80,
    });

    let mut second_virtual_model = first_virtual_model.clone();
    second_virtual_model.id = "custom-model-2".to_string();
    second_virtual_model.host_model_id = Some("MODEL_PLACEHOLDER_M401".to_string());
    second_virtual_model.upstream_model_id = "upstream-model-2".to_string();
    let mut second_upstream_model = first_upstream_model.clone();
    second_upstream_model.id = "upstream-model-2".to_string();
    second_upstream_model.upstream_model_id = "provider-model-2".to_string();
    second_upstream_model.checkpoint_override = None;

    let settings = OfficialModelSettings {
        custom_model: custom_compression(CustomModelCompressionProfile::Custom, 60, 80, 5),
        ..OfficialModelSettings::default()
    };
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[first_virtual_model, second_virtual_model],
        &[first_upstream_model, second_upstream_model],
        &settings,
    );

    let first_checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(first_checkpoint["token_threshold"], "238080");
    assert_eq!(first_checkpoint["max_token_limit"], "297600");
    assert_eq!(first_checkpoint["max_output_tokens"], "18600");

    let second_checkpoint = checkpoint(&catalog["models"]["custom-model-2"]);
    assert_eq!(second_checkpoint["token_threshold"], "223200");
    assert_eq!(second_checkpoint["max_token_limit"], "297600");
    assert_eq!(second_checkpoint["max_output_tokens"], "18600");
}

#[test]
fn custom_model_override_replaces_global_values_and_is_safely_clipped() {
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
    let settings = OfficialModelSettings::default();
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    let checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(checkpoint["token_threshold"], "190000");
    assert_eq!(checkpoint["max_token_limit"], "200000");
    assert_eq!(checkpoint["max_output_tokens"], "10000");
}

#[test]
fn applies_custom_model_percentage_profile_for_200k_effective_limit() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(200_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(32_000),
        ..ModelTokenLimits::default()
    };
    let settings = OfficialModelSettings {
        custom_model: custom_compression(CustomModelCompressionProfile::Custom, 70, 90, 5),
        ..OfficialModelSettings::default()
    };
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    let checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(checkpoint["token_threshold"], "140000");
    assert_eq!(checkpoint["max_token_limit"], "180000");
    assert_eq!(checkpoint["max_output_tokens"], "10000");
}

#[test]
fn scales_explicit_balanced_custom_model_profile_to_effective_context_limit() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(200_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(32_000),
        ..ModelTokenLimits::default()
    };
    let settings = OfficialModelSettings {
        custom_model: custom_compression(CustomModelCompressionProfile::Balanced, 61, 73, 2),
        ..OfficialModelSettings::default()
    };
    let mut catalog = json!({ "models": {} });

    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    let checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(checkpoint["token_threshold"], "122070");
    assert_eq!(checkpoint["max_token_limit"], "146484");
    assert_eq!(checkpoint["max_output_tokens"], "3125");
}

#[test]
fn prefers_catalog_capacity_over_existing_claude_checkpoint_for_safe_profile() {
    let existing_checkpoint = json!({
        "token_threshold": "120000",
        "max_token_limit": "150000",
        "max_output_tokens": "16000",
        "checkpoint_model": "MODEL_CLAUDE_SONNET"
    });
    let mut catalog = json!({
        "models": {
            "claude-sonnet": {
                "model": "MODEL_CLAUDE_SONNET",
                "maxTokens": 200_000,
                "contextWindow": 200_000,
                "maxOutputTokens": 32_000,
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": serde_json::to_string(&existing_checkpoint).unwrap()
                        }
                    }
                }
            }
        }
    });
    let settings = OfficialModelSettings {
        claude: official_compression(OfficialCompressionProfile::Safe),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    let checkpoint = checkpoint(&catalog["models"]["claude-sonnet"]);
    assert_eq!(checkpoint["token_threshold"], "82015");
    assert_eq!(checkpoint["max_token_limit"], "97656");
    assert_eq!(checkpoint["max_output_tokens"], "3125");
}

#[test]
fn applies_relative_claude_profiles_for_200k_catalog_capacity() {
    for (profile, expected) in [
        (OfficialCompressionProfile::Safe, (82_015, 97_656, 3_125)),
        (
            OfficialCompressionProfile::Balanced,
            (122_070, 146_484, 3_125),
        ),
        (
            OfficialCompressionProfile::Aggressive,
            (144_958, 171_661, 3_125),
        ),
    ] {
        let mut catalog = json!({
            "models": {
                "claude-sonnet": {
                    "model": "MODEL_CLAUDE_SONNET",
                    "displayName": "Claude Sonnet",
                    "maxTokens": 200_000,
                    "contextWindow": 200_000,
                    "maxOutputTokens": 32_000
                }
            }
        });
        let settings = OfficialModelSettings {
            claude: official_compression(profile),
            ..OfficialModelSettings::default()
        };

        AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

        let checkpoint = checkpoint(&catalog["models"]["claude-sonnet"]);
        let threshold = checkpoint["token_threshold"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let hard_limit = checkpoint["max_token_limit"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let output_reserve = checkpoint["max_output_tokens"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        assert_eq!((threshold, hard_limit, output_reserve), expected);
        assert!(threshold + output_reserve <= hard_limit);
        assert!(hard_limit <= 200_000);
    }
}

#[test]
fn applies_custom_claude_percentages_for_200k_catalog_capacity() {
    let existing_checkpoint = json!({
        "token_threshold": "80000",
        "max_token_limit": "100000",
        "max_output_tokens": "16000",
        "checkpoint_model": "MODEL_CLAUDE_SONNET"
    });
    let mut catalog = json!({
        "models": {
            "claude-sonnet": {
                "model": "MODEL_CLAUDE_SONNET",
                "maxTokens": 200_000,
                "contextWindow": 200_000,
                "maxOutputTokens": 32_000,
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": serde_json::to_string(&existing_checkpoint).unwrap()
                        }
                    }
                }
            },
            "claude-without-capacity": {
                "model": "MODEL_CLAUDE_UNKNOWN"
            }
        }
    });
    let settings = OfficialModelSettings {
        claude: OfficialCompressionSettings {
            profile: OfficialCompressionProfile::Custom,
            percentages: CompressionPercentages {
                token_threshold: 70,
                max_token_limit: 90,
                max_output_tokens: 5,
            },
        },
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    let checkpoint = checkpoint(&catalog["models"]["claude-sonnet"]);
    assert_eq!(checkpoint["token_threshold"], "140000");
    assert_eq!(checkpoint["max_token_limit"], "180000");
    assert_eq!(checkpoint["max_output_tokens"], "10000");
    assert!(catalog["models"]["claude-without-capacity"]
        .get("modelExperiments")
        .is_none());
}

#[test]
fn identifies_families_in_array_catalogs_and_skips_ambiguous_or_capacityless_claude() {
    let mut catalog = json!([
        {
            "id": "gemini-pro",
            "model": "MODEL_GEMINI_PRO"
        },
        {
            "id": "claude-sonnet",
            "model": "MODEL_CLAUDE_SONNET",
            "inputTokenLimit": 200_000,
            "contextWindow": 220_000
        },
        {
            "id": "ambiguous",
            "model": "MODEL_GEMINI_CLAUDE",
            "maxTokens": 200_000
        },
        {
            "id": "claude-without-capacity",
            "model": "MODEL_CLAUDE_UNKNOWN"
        }
    ]);
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Safe),
        claude: official_compression(OfficialCompressionProfile::Safe),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    let gemini_checkpoint = checkpoint(&catalog[0]);
    assert_eq!(gemini_checkpoint["max_token_limit"], "512000");
    let claude_checkpoint = checkpoint(&catalog[1]);
    assert_eq!(claude_checkpoint["max_token_limit"], "97656");
    assert!(catalog[2].get("modelExperiments").is_none());
    assert!(catalog[3].get("modelExperiments").is_none());
}

#[test]
fn distinguishes_official_and_custom_placeholder_ranges() {
    let mut catalog = json!({
        "models": {
            "MODEL_PLACEHOLDER_M50": {
                "displayName": "Gemini Checkpoint",
                "model": "MODEL_PLACEHOLDER_M50"
            },
            "MODEL_PLACEHOLDER_M400": {
                "displayName": "Custom Gemini",
                "model": "MODEL_PLACEHOLDER_M400"
            }
        }
    });
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Safe),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    assert_eq!(
        checkpoint(&catalog["models"]["MODEL_PLACEHOLDER_M50"])["max_token_limit"],
        "512000"
    );
    assert!(catalog["models"]["MODEL_PLACEHOLDER_M400"]
        .get("modelExperiments")
        .is_none());
}

#[test]
fn keeps_gemini_claude_and_custom_model_profiles_independent() {
    let (virtual_model, mut upstream_model) = models();
    upstream_model.token_limits = ModelTokenLimits {
        context_window: Some(372_000),
        input_token_limit: Some(372_000),
        output_token_limit: Some(128_000),
        ..ModelTokenLimits::default()
    };
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Safe),
        claude: official_compression(OfficialCompressionProfile::Balanced),
        custom_model: custom_compression(CustomModelCompressionProfile::Custom, 40, 60, 5),
    };
    let mut catalog = json!({
        "models": {
            "gemini-pro": {
                "model": "MODEL_GEMINI_PRO"
            },
            "claude-sonnet": {
                "model": "MODEL_CLAUDE_SONNET",
                "maxTokens": 200_000,
                "contextWindow": 200_000,
                "maxOutputTokens": 32_000
            },
            "native-model": {
                "model": "MODEL_NATIVE"
            }
        }
    });

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);
    AntigravityModelDescriptor::inject_into_model_list_with_settings(
        &mut catalog,
        &[virtual_model],
        &[upstream_model],
        &settings,
    );

    let gemini_checkpoint = checkpoint(&catalog["models"]["gemini-pro"]);
    assert_eq!(gemini_checkpoint["token_threshold"], "430000");
    assert_eq!(gemini_checkpoint["max_token_limit"], "512000");
    assert_eq!(gemini_checkpoint["max_output_tokens"], "16384");

    let claude_checkpoint = checkpoint(&catalog["models"]["claude-sonnet"]);
    assert_eq!(claude_checkpoint["token_threshold"], "122070");
    assert_eq!(claude_checkpoint["max_token_limit"], "146484");
    assert_eq!(claude_checkpoint["max_output_tokens"], "3125");

    let custom_checkpoint = checkpoint(&catalog["models"]["custom-model"]);
    assert_eq!(custom_checkpoint["token_threshold"], "148800");
    assert_eq!(custom_checkpoint["max_token_limit"], "223200");
    assert_eq!(custom_checkpoint["max_output_tokens"], "18600");
    assert!(catalog["models"]["native-model"]
        .get("modelExperiments")
        .is_none());
}

#[test]
fn leaves_claude_checkpoint_unchanged_when_catalog_capacity_is_missing() {
    let existing_checkpoint = json!({
        "token_threshold": "120000",
        "max_token_limit": "150000",
        "max_output_tokens": "16000",
        "checkpoint_model": "MODEL_CLAUDE_SONNET"
    });
    let mut catalog = json!({
        "models": {
            "claude-sonnet": {
                "model": "MODEL_CLAUDE_SONNET",
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": serde_json::to_string(&existing_checkpoint).unwrap()
                        }
                    }
                }
            }
        }
    });
    let settings = OfficialModelSettings {
        claude: official_compression(OfficialCompressionProfile::Safe),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    let checkpoint = checkpoint(&catalog["models"]["claude-sonnet"]);
    assert_eq!(checkpoint["token_threshold"], "120000");
    assert_eq!(checkpoint["max_token_limit"], "150000");
    assert_eq!(checkpoint["max_output_tokens"], "16000");
}

#[test]
fn preserves_existing_checkpoint_model_for_opaque_claude_catalog_keys() {
    let existing_checkpoint = json!({
        "token_threshold": "120000",
        "max_token_limit": "150000",
        "max_output_tokens": "16000",
        "checkpoint_model": "MODEL_CLAUDE_SONNET"
    });
    let mut catalog = json!({
        "models": {
            "opaque-entry": {
                "maxTokens": 200_000,
                "modelExperiments": {
                    "experiments": {
                        "CASCADE_USE_EXPERIMENT_CHECKPOINTER": {
                            "stringValue": serde_json::to_string(&existing_checkpoint).unwrap()
                        }
                    }
                }
            }
        }
    });
    let settings = OfficialModelSettings {
        claude: official_compression(OfficialCompressionProfile::Safe),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    assert_eq!(
        checkpoint(&catalog["models"]["opaque-entry"])["checkpoint_model"],
        "MODEL_CLAUDE_SONNET"
    );
}

#[test]
fn applies_selected_checkpoint_profile_only_to_official_gemini_models() {
    let mut catalog = json!({
        "models": {
            "gemini-pro": {
                "model": "MODEL_GEMINI_2_5_PRO",
                "displayName": "Gemini Pro"
            },
            "native-model": {
                "model": "MODEL_NATIVE",
                "displayName": "Native Model"
            }
        }
    });
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Balanced),
        ..OfficialModelSettings::default()
    };

    AntigravityModelDescriptor::apply_official_model_overrides(&mut catalog, &settings);

    let raw = catalog["models"]["gemini-pro"]["modelExperiments"]["experiments"]
        ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"]
        .as_str()
        .unwrap();
    let checkpoint: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(checkpoint["token_threshold"], "640000");
    assert_eq!(checkpoint["max_token_limit"], "768000");
    assert_eq!(checkpoint["max_output_tokens"], "16384");
    assert!(catalog["models"]["native-model"]
        .get("modelExperiments")
        .is_none());
}
