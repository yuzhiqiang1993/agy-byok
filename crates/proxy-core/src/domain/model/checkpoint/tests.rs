use super::*;

const fn percentages(
    token_threshold: u8,
    max_token_limit: u8,
    max_output_tokens: u8,
) -> CompressionPercentages {
    CompressionPercentages {
        token_threshold,
        max_token_limit,
        max_output_tokens,
    }
}

const fn checkpoint(
    token_threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) -> CheckpointLimits {
    CheckpointLimits::new(token_threshold, max_token_limit, max_output_tokens)
}

const fn official_compression(
    profile: OfficialCompressionProfile,
    percentages: CompressionPercentages,
) -> OfficialCompressionSettings {
    OfficialCompressionSettings {
        profile,
        percentages,
    }
}

const fn custom_compression(
    profile: CustomModelCompressionProfile,
    percentages: CompressionPercentages,
) -> CustomModelCompressionSettings {
    CustomModelCompressionSettings {
        profile,
        percentages,
    }
}

#[test]
fn checkpoint_override_validation_enforces_contract() {
    assert!(ModelCheckpointOverride::Percentage {
        threshold_percent: 1,
    }
    .validate()
    .is_ok());
    assert!(ModelCheckpointOverride::Percentage {
        threshold_percent: 100,
    }
    .validate()
    .is_ok());
    assert!(ModelCheckpointOverride::Custom {
        token_threshold: 80,
        max_token_limit: 100,
        max_output_tokens: 20,
    }
    .validate()
    .is_ok());

    for invalid in [
        ModelCheckpointOverride::Percentage {
            threshold_percent: 0,
        },
        ModelCheckpointOverride::Percentage {
            threshold_percent: 101,
        },
        ModelCheckpointOverride::Custom {
            token_threshold: 0,
            max_token_limit: 100,
            max_output_tokens: 20,
        },
        ModelCheckpointOverride::Custom {
            token_threshold: 100,
            max_token_limit: 100,
            max_output_tokens: 1,
        },
        ModelCheckpointOverride::Custom {
            token_threshold: 1,
            max_token_limit: 100,
            max_output_tokens: 100,
        },
        ModelCheckpointOverride::Custom {
            token_threshold: 80,
            max_token_limit: 100,
            max_output_tokens: 30,
        },
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

#[test]
fn official_model_settings_default_to_independent_profiles_and_percentages() {
    let settings = OfficialModelSettings::default();

    assert_eq!(
        settings.gemini.profile,
        OfficialCompressionProfile::Official
    );
    assert_eq!(
        settings.claude.profile,
        OfficialCompressionProfile::Official
    );
    assert_eq!(
        settings.custom_model.profile,
        CustomModelCompressionProfile::None
    );
    assert_eq!(
        [
            settings.gemini.percentages,
            settings.claude.percentages,
            settings.custom_model.percentages,
        ],
        [CompressionPercentages::default(); 3]
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 200_000, 32_000),
        None
    );
}

#[test]
fn custom_model_none_profile_round_trips_without_checkpoint_limits() {
    let settings = OfficialModelSettings::default();
    let value = serde_json::to_value(&settings).unwrap();

    assert_eq!(value["custom_model"]["profile"], "none");
    assert_eq!(
        serde_json::from_value::<OfficialModelSettings>(value).unwrap(),
        settings
    );
}

#[test]
fn compression_profiles_and_percentages_round_trip_with_snake_case_schema() {
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Custom, percentages(70, 90, 5)),
        claude: official_compression(OfficialCompressionProfile::Custom, percentages(70, 90, 5)),
        custom_model: custom_compression(
            CustomModelCompressionProfile::Custom,
            percentages(65, 85, 4),
        ),
    };

    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(value["gemini"]["profile"], "custom");
    assert_eq!(value["gemini"]["percentages"]["token_threshold"], 70);
    assert_eq!(value["claude"]["profile"], "custom");
    assert_eq!(value["claude"]["percentages"]["max_token_limit"], 90);
    assert_eq!(value["custom_model"]["profile"], "custom");
    assert_eq!(value["custom_model"]["percentages"]["max_output_tokens"], 4);
    assert_eq!(value.as_object().unwrap().len(), 3);
    assert_eq!(
        serde_json::from_value::<OfficialModelSettings>(value).unwrap(),
        settings
    );

    let old_flat_schema = serde_json::json!({
        "gemini_compression_profile": "custom",
        "gemini_token_threshold_percent": 70,
        "gemini_max_token_limit_percent": 90,
        "gemini_max_output_tokens_percent": 5
    });
    assert!(serde_json::from_value::<OfficialModelSettings>(old_flat_schema).is_err());
}

#[test]
fn custom_claude_profile_scales_capacity_and_safely_clips_limits() {
    let metadata = ClaudeCheckpointMetadata {
        capacity: 200_000,
        output_token_limit: Some(32_000),
    };
    let defaults = OfficialModelSettings {
        claude: official_compression(
            OfficialCompressionProfile::Custom,
            CompressionPercentages::default(),
        ),
        ..OfficialModelSettings::default()
    };
    assert_eq!(
        defaults.claude_checkpoint_limits(metadata),
        Some(checkpoint(122_000, 146_000, 4_000))
    );

    let configured = OfficialModelSettings {
        claude: official_compression(OfficialCompressionProfile::Custom, percentages(70, 90, 5)),
        ..OfficialModelSettings::default()
    };
    assert_eq!(
        configured.claude_checkpoint_limits(metadata),
        Some(checkpoint(140_000, 180_000, 10_000))
    );
    assert_eq!(
        configured.claude_checkpoint_limits(ClaudeCheckpointMetadata {
            output_token_limit: Some(8_000),
            ..metadata
        }),
        Some(checkpoint(140_000, 180_000, 8_000))
    );
    assert_eq!(
        configured.claude_checkpoint_limits(ClaudeCheckpointMetadata {
            capacity: 0,
            ..metadata
        }),
        None
    );
}

#[test]
fn catalog_capacity_claude_presets_ignore_existing_checkpoint_values() {
    let metadata = ClaudeCheckpointMetadata {
        capacity: 200_000,
        output_token_limit: Some(32_000),
    };
    let settings = OfficialModelSettings {
        claude: official_compression(
            OfficialCompressionProfile::Safe,
            CompressionPercentages::default(),
        ),
        ..OfficialModelSettings::default()
    };

    assert_eq!(
        settings.claude_checkpoint_limits(metadata),
        Some(checkpoint(82_015, 97_656, 3_125))
    );
}

#[test]
fn validates_claude_percentage_triplets() {
    assert!(OfficialModelSettings::default().validate().is_ok());

    for (threshold_percent, max_limit_percent, max_output_percent) in [
        (0, 73, 2),
        (101, 73, 2),
        (61, 0, 2),
        (61, 101, 2),
        (61, 73, 0),
        (61, 73, 101),
        (73, 73, 2),
        (61, 73, 73),
        (70, 73, 4),
    ] {
        let settings = OfficialModelSettings {
            claude: official_compression(
                OfficialCompressionProfile::Custom,
                percentages(threshold_percent, max_limit_percent, max_output_percent),
            ),
            ..OfficialModelSettings::default()
        };
        assert!(
            settings.validate().is_err(),
            "unexpected valid Claude percentages: {threshold_percent}/{max_limit_percent}/{max_output_percent}"
        );
    }
}

#[test]
fn gemini_presets_use_fixed_limits() {
    for (profile, expected) in [
        (
            OfficialCompressionProfile::Safe,
            checkpoint(430_000, 512_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
        ),
        (
            OfficialCompressionProfile::Balanced,
            checkpoint(640_000, 768_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
        ),
        (
            OfficialCompressionProfile::Aggressive,
            checkpoint(760_000, 900_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
        ),
    ] {
        let settings = OfficialModelSettings {
            gemini: official_compression(profile, CompressionPercentages::default()),
            ..OfficialModelSettings::default()
        };

        assert_eq!(settings.gemini_checkpoint_limits(), Some(expected));
        assert!(settings.validate().is_ok());
    }
}

#[test]
fn gemini_custom_percentages_scale_from_context_window() {
    let settings = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Custom, percentages(70, 90, 5)),
        ..OfficialModelSettings::default()
    };

    assert_eq!(
        settings.gemini_checkpoint_limits(),
        Some(checkpoint(734_003, 943_718, 52_428))
    );
    assert!(settings.validate().is_ok());
}

#[test]
fn custom_model_custom_profile_scales_three_percentages() {
    let settings = OfficialModelSettings {
        custom_model: custom_compression(
            CustomModelCompressionProfile::Custom,
            percentages(70, 90, 5),
        ),
        ..OfficialModelSettings::default()
    };
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 200_000, 32_000),
        Some(checkpoint(140_000, 180_000, 10_000))
    );
    assert!(settings.validate().is_ok());
}

#[test]
fn validates_gemini_and_custom_model_percentage_fields() {
    for (threshold, max_limit, max_output) in [
        (0, 90, 5),
        (101, 90, 5),
        (70, 0, 5),
        (70, 101, 5),
        (70, 90, 0),
        (70, 90, 101),
        (90, 90, 5),
        (70, 90, 90),
        (70, 73, 4),
    ] {
        let gemini = OfficialModelSettings {
            gemini: official_compression(
                OfficialCompressionProfile::Custom,
                percentages(threshold, max_limit, max_output),
            ),
            ..OfficialModelSettings::default()
        };
        assert!(
            gemini.validate().is_err(),
            "unexpected valid Gemini percentages: {threshold}/{max_limit}/{max_output}"
        );

        let custom = OfficialModelSettings {
            custom_model: custom_compression(
                CustomModelCompressionProfile::Custom,
                percentages(threshold, max_limit, max_output),
            ),
            ..OfficialModelSettings::default()
        };
        assert!(
            custom.validate().is_err(),
            "unexpected valid custom-model percentages: {threshold}/{max_limit}/{max_output}"
        );
    }

    let inactive_profiles_with_invalid_percentages = OfficialModelSettings {
        gemini: official_compression(OfficialCompressionProfile::Balanced, percentages(0, 0, 0)),
        custom_model: custom_compression(
            CustomModelCompressionProfile::Balanced,
            percentages(0, 0, 0),
        ),
        ..OfficialModelSettings::default()
    };
    assert!(inactive_profiles_with_invalid_percentages
        .validate()
        .is_err());
}

#[test]
fn custom_model_presets_scale_relative_to_effective_input_limit() {
    for (profile, expected) in [
        (
            CustomModelCompressionProfile::Safe,
            checkpoint(82_015, 97_656, 3_125),
        ),
        (
            CustomModelCompressionProfile::Balanced,
            checkpoint(122_070, 146_484, 3_125),
        ),
        (
            CustomModelCompressionProfile::Aggressive,
            checkpoint(144_958, 171_661, 3_125),
        ),
    ] {
        let settings = OfficialModelSettings {
            custom_model: custom_compression(profile, CompressionPercentages::default()),
            ..OfficialModelSettings::default()
        };

        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(None, 200_000, 32_000),
            Some(expected)
        );
    }
}

#[test]
fn custom_model_profile_preserves_checkpoint_override_priority() {
    let settings = OfficialModelSettings {
        custom_model: custom_compression(
            CustomModelCompressionProfile::Balanced,
            CompressionPercentages::default(),
        ),
        ..OfficialModelSettings::default()
    };
    let percentage = ModelCheckpointOverride::Percentage {
        threshold_percent: 80,
    };
    let custom = ModelCheckpointOverride::Custom {
        token_threshold: 150_000,
        max_token_limit: 180_000,
        max_output_tokens: 10_000,
    };

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 200_000, 32_000),
        Some(checkpoint(122_070, 146_484, 3_125))
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&percentage), 200_000, 32_000),
        Some(checkpoint(117_187, 146_484, 3_125))
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&custom), 200_000, 32_000,),
        Some(checkpoint(150_000, 180_000, 10_000))
    );
}

#[test]
fn explicit_percentage_override_enables_checkpoint_when_global_profile_is_none() {
    let settings = OfficialModelSettings::default();
    let percentage = ModelCheckpointOverride::Percentage {
        threshold_percent: 80,
    };

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&percentage), 200_000, 32_000),
        Some(checkpoint(116_800, 146_000, 4_000))
    );
}

#[test]
fn checkpoint_resolution_honors_override_priority_and_safety_clipping() {
    let settings = OfficialModelSettings {
        custom_model: custom_compression(
            CustomModelCompressionProfile::Custom,
            percentages(60, 80, 5),
        ),
        ..OfficialModelSettings::default()
    };
    let percentage = ModelCheckpointOverride::Percentage {
        threshold_percent: 80,
    };
    let custom = ModelCheckpointOverride::Custom {
        token_threshold: 250_000,
        max_token_limit: 300_000,
        max_output_tokens: 20_000,
    };

    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(None, 372_000, 128_000),
        Some(checkpoint(223_200, 297_600, 18_600))
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&percentage), 372_000, 128_000,),
        Some(checkpoint(238_080, 297_600, 18_600))
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&custom), 372_000, 128_000,),
        Some(checkpoint(250_000, 300_000, 20_000))
    );
    assert_eq!(
        settings.custom_model_checkpoint_limits_with_override(Some(&custom), 200_000, 10_000,),
        Some(checkpoint(190_000, 200_000, 10_000))
    );
}
