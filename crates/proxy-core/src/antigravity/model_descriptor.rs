use crate::domain::{OfficialModelSettings, ReasoningMapping, UpstreamModel, VirtualModel};
use serde_json::{json, Map, Value};

// 供应商目录没有提供限制时使用保守的经验回退值；它不会写回模型配置。
// 只要目录返回了模型级限制，token_limits() 就会优先使用真实值。
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const DEFAULT_INPUT_TOKEN_LIMIT: u32 = 128_000;
const DEFAULT_OUTPUT_TOKEN_LIMIT: u32 = 128_000;

pub struct AntigravityModelDescriptor;

impl AntigravityModelDescriptor {
    pub fn build_model_object(
        virtual_model: &VirtualModel,
        upstream_model: &UpstreamModel,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let host_model_id = virtual_model.effective_host_model_id().into_owned();
        let supported_mime_types = supported_mime_types(caps.vision);
        let (context_window, input_token_limit, output_token_limit) = token_limits(upstream_model);

        let mut descriptor = json!({
            "id": virtual_model.id,
            "name": format!("models/{host_model_id}"),
            "displayName": virtual_model.display_name,
            "description": format!("Custom BYOK Model (Provider: {})", upstream_model.provider_id),
            "contextWindow": context_window,
            "inputTokenLimit": input_token_limit,
            "outputTokenLimit": output_token_limit,
            "supportsImages": caps.vision,
            "supportsTools": caps.tools,
            "supportsThinking": caps.reasoning.supports_reasoning(),
            "supportedMimeTypes": supported_mime_types.keys().collect::<Vec<_>>()
        });
        apply_reasoning_metadata(&mut descriptor, virtual_model, upstream_model);
        descriptor
    }

    pub fn build_cloud_code_catalog_entry(
        virtual_model: &VirtualModel,
        upstream_model: &UpstreamModel,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let host_model_id = virtual_model.effective_host_model_id().into_owned();
        let input_modalities = if caps.vision {
            vec!["IMAGE", "TEXT"]
        } else {
            vec!["TEXT"]
        };
        let input_modalities_lowercase = if caps.vision {
            vec!["image", "text"]
        } else {
            vec!["text"]
        };
        let (context_window, input_token_limit, output_token_limit) = token_limits(upstream_model);

        let mut descriptor = json!({
            "displayName": virtual_model.display_name,
            // Antigravity 的 maxTokens 是 planner 输入预算，不是请求的输出参数。
            "contextWindow": context_window,
            "maxTokens": input_token_limit,
            "maxOutputTokens": output_token_limit,
            "model": host_model_id,
            "planModel": host_model_id,
            "requestedModel": host_model_id,
            "apiProvider": "API_PROVIDER_GOOGLE_GEMINI",
            "modelProvider": "MODEL_PROVIDER_GOOGLE",
            "recommended": true,
            "supportsImages": caps.vision,
            "supportsVision": caps.vision,
            "supportsImage": caps.vision,
            "supportsThinking": caps.reasoning.supports_reasoning(),
            "supportsVideo": false,
            "inputModalities": input_modalities,
            "input_modalities": input_modalities_lowercase,
            "supportedMimeTypes": supported_mime_types(caps.vision),
            "tokenizerType": "LLAMA_WITH_SPECIAL"
        });
        apply_reasoning_metadata(&mut descriptor, virtual_model, upstream_model);
        descriptor
    }

    pub fn inject_into_model_list(
        models_json: &mut Value,
        virtual_models: &[VirtualModel],
        upstream_models: &[UpstreamModel],
    ) {
        Self::inject_into_model_list_with_settings(
            models_json,
            virtual_models,
            upstream_models,
            &OfficialModelSettings::default(),
        );
    }

    pub fn inject_into_model_list_with_settings(
        models_json: &mut Value,
        virtual_models: &[VirtualModel],
        upstream_models: &[UpstreamModel],
        settings: &OfficialModelSettings,
    ) {
        let models = virtual_models
            .iter()
            .filter(|virtual_model| virtual_model.enabled)
            .filter_map(|virtual_model| {
                upstream_models
                    .iter()
                    .find(|upstream_model| {
                        upstream_model.id == virtual_model.upstream_model_id
                            && upstream_model.enabled
                    })
                    .map(|upstream_model| (virtual_model, upstream_model))
            })
            .collect::<Vec<_>>();

        if let Some(catalog) = models_json.as_object_mut() {
            if let Some(target) = catalog.get_mut("models") {
                let catalog_keys = (!target.is_array()).then(|| {
                    models
                        .iter()
                        .map(|(virtual_model, _)| virtual_model.catalog_key().into_owned())
                        .collect::<Vec<_>>()
                });
                inject_models(target, models, settings);
                if let Some(catalog_keys) = catalog_keys {
                    append_catalog_keys_to_model_sorts(
                        catalog.get_mut("agentModelSorts"),
                        &catalog_keys,
                    );
                }
                return;
            }
        }

        inject_models(models_json, models, settings);
    }

    /// 覆盖官方 Gemini 模型目录中的检查点参数。
    ///
    /// Antigravity IDE 会从 `modelExperiments` 中读取检查点策略；这与实际
    /// 生成请求的 `max_tokens` 不是同一层配置。官方档位不做任何改写，避免
    /// 上游将来的参数变化被本地默认值遮蔽。
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        settings: &OfficialModelSettings,
    ) {
        let Some((threshold, max_token_limit, max_output_tokens)) =
            settings.gemini_checkpoint_limits()
        else {
            return;
        };

        if models_json.get("models").is_some() {
            if let Some(models) = models_json.get_mut("models") {
                apply_checkpoint_overrides_to_models(
                    models,
                    threshold,
                    max_token_limit,
                    max_output_tokens,
                );
            }
        } else {
            apply_checkpoint_overrides_to_models(
                models_json,
                threshold,
                max_token_limit,
                max_output_tokens,
            );
        }
    }
}

fn apply_checkpoint_overrides_to_models(
    models: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) {
    match models {
        Value::Object(entries) => {
            for (key, entry) in entries {
                if is_official_gemini_model(Some(key), entry) {
                    apply_checkpoint_override(entry, threshold, max_token_limit, max_output_tokens);
                }
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                if is_official_gemini_model(None, entry) {
                    apply_checkpoint_override(entry, threshold, max_token_limit, max_output_tokens);
                }
            }
        }
        _ => {}
    }
}

fn is_official_gemini_model(key: Option<&str>, entry: &Value) -> bool {
    let mut candidates = Vec::with_capacity(7);
    if let Some(key) = key {
        candidates.push(key);
    }
    for field in [
        "id",
        "model",
        "modelId",
        "requestedModel",
        "displayName",
        "name",
    ] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            candidates.push(value);
        }
    }

    candidates.into_iter().any(|value| {
        let normalized = value.to_ascii_lowercase();
        normalized.contains("gemini") || normalized.contains("model_gemini")
    })
}

fn apply_checkpoint_override(
    entry: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) {
    let checkpoint_model = ["model", "modelId", "requestedModel", "id"]
        .iter()
        .filter_map(|field| entry.get(*field).and_then(Value::as_str))
        .next()
        .unwrap_or("MODEL_GEMINI")
        .to_string();
    apply_checkpoint_override_with_model(
        entry,
        threshold,
        max_token_limit,
        max_output_tokens,
        &checkpoint_model,
    );
}

fn apply_checkpoint_override_with_model(
    entry: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
    checkpoint_model: &str,
) {
    let Some(entry_object) = entry.as_object_mut() else {
        return;
    };
    let experiment = entry_object
        .entry("modelExperiments")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .map(|experiments| {
            experiments
                .entry("experiments")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .flatten()
        .map(|experiments| {
            experiments
                .entry("CASCADE_USE_EXPERIMENT_CHECKPOINTER")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .flatten();
    let Some(experiment) = experiment else {
        return;
    };

    let mut checkpoint = experiment
        .get("stringValue")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let Some(checkpoint) = checkpoint.as_object_mut() else {
        return;
    };
    checkpoint.insert(
        "strategy".to_string(),
        Value::String("CHECKPOINT_STRATEGY_UNSPECIFIED".to_string()),
    );
    checkpoint.insert(
        "token_threshold".to_string(),
        Value::String(threshold.to_string()),
    );
    checkpoint.insert(
        "max_token_limit".to_string(),
        Value::String(max_token_limit.to_string()),
    );
    checkpoint.insert(
        "max_output_tokens".to_string(),
        Value::String(max_output_tokens.to_string()),
    );
    checkpoint
        .entry("max_overhead_ratio")
        .or_insert_with(|| json!(0.15));
    checkpoint
        .entry("moving_window_size")
        .or_insert_with(|| json!(1));
    checkpoint.insert(
        "checkpoint_model".to_string(),
        Value::String(checkpoint_model.to_string()),
    );
    if let Ok(serialized) = serde_json::to_string(&Value::Object(checkpoint.clone())) {
        experiment.insert("stringValue".to_string(), Value::String(serialized));
    }
}

fn token_limits(upstream_model: &UpstreamModel) -> (u32, u32, u32) {
    (
        upstream_model
            .token_limits
            .context_window
            .unwrap_or(DEFAULT_CONTEXT_WINDOW),
        upstream_model
            .token_limits
            .input_token_limit
            .unwrap_or(DEFAULT_INPUT_TOKEN_LIMIT),
        upstream_model
            .token_limits
            .output_token_limit
            .unwrap_or(DEFAULT_OUTPUT_TOKEN_LIMIT),
    )
}

fn apply_reasoning_metadata(
    descriptor: &mut Value,
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
) {
    if !upstream_model.capabilities.reasoning.supports_reasoning() {
        return;
    }
    let Some(descriptor) = descriptor.as_object_mut() else {
        return;
    };
    let Some(level) = virtual_model.default_reasoning_level else {
        return;
    };
    let Some(mapping) = upstream_model.capabilities.reasoning.mapping_for(level) else {
        return;
    };

    let reasoning_effort = match mapping {
        ReasoningMapping::Effort(value) | ReasoningMapping::NativeLevel(value) => {
            Value::String(value.clone())
        }
        _ => serde_json::to_value(level).expect("reasoning level serialization cannot fail"),
    };
    descriptor.insert("reasoningEffort".to_string(), reasoning_effort);

    match mapping {
        ReasoningMapping::BudgetTokens(tokens) => {
            descriptor.insert("thinkingBudget".to_string(), json!(tokens));
        }
        ReasoningMapping::Disabled => {
            descriptor.insert("thinkingBudget".to_string(), json!(0));
        }
        ReasoningMapping::Adaptive
        | ReasoningMapping::Effort(_)
        | ReasoningMapping::NativeLevel(_) => {}
    }
}

fn apply_custom_model_checkpoint_override(
    descriptor: &mut Value,
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
    settings: &OfficialModelSettings,
) {
    let (context_window, input_token_limit, output_token_limit) = token_limits(upstream_model);
    let checkpoint_token_limit = context_window.min(input_token_limit);
    let Some((threshold, max_token_limit, max_output_tokens)) = settings
        .custom_model_checkpoint_limits_with_override(
            upstream_model.checkpoint_override.as_ref(),
            checkpoint_token_limit,
            output_token_limit,
        )
    else {
        return;
    };
    let checkpoint_model = virtual_model.effective_host_model_id();
    apply_checkpoint_override_with_model(
        descriptor,
        threshold,
        max_token_limit,
        max_output_tokens,
        checkpoint_model.as_ref(),
    );
}

fn custom_model_object(
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
    settings: &OfficialModelSettings,
) -> Value {
    let mut descriptor =
        AntigravityModelDescriptor::build_model_object(virtual_model, upstream_model);
    apply_custom_model_checkpoint_override(
        &mut descriptor,
        virtual_model,
        upstream_model,
        settings,
    );
    descriptor
}

fn custom_cloud_code_catalog_entry(
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
    settings: &OfficialModelSettings,
) -> Value {
    let mut descriptor =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(virtual_model, upstream_model);
    apply_custom_model_checkpoint_override(
        &mut descriptor,
        virtual_model,
        upstream_model,
        settings,
    );
    descriptor
}

fn inject_models(
    target: &mut Value,
    models: Vec<(&VirtualModel, &UpstreamModel)>,
    settings: &OfficialModelSettings,
) {
    match target {
        Value::Array(entries) => {
            entries.extend(models.into_iter().map(|(virtual_model, upstream_model)| {
                custom_model_object(virtual_model, upstream_model, settings)
            }));
        }
        Value::Object(entries) => {
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(virtual_model, upstream_model, settings),
                );
            }
        }
        _ => {
            let mut entries = Map::new();
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(virtual_model, upstream_model, settings),
                );
            }
            *target = Value::Object(entries);
        }
    }
}

fn append_catalog_keys_to_model_sorts(model_sorts: Option<&mut Value>, catalog_keys: &[String]) {
    let Some(model_sorts) = model_sorts.and_then(Value::as_array_mut) else {
        return;
    };

    for model_sort in model_sorts {
        let Some(groups) = model_sort.get_mut("groups").and_then(Value::as_array_mut) else {
            continue;
        };

        for group in groups {
            let Some(model_ids) = group.get_mut("modelIds").and_then(Value::as_array_mut) else {
                continue;
            };

            for catalog_key in catalog_keys {
                if !model_ids
                    .iter()
                    .any(|model_id| model_id.as_str() == Some(catalog_key.as_str()))
                {
                    model_ids.push(Value::String(catalog_key.clone()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ModelCapabilities, ModelCheckpointOverride, ModelTokenLimits, OfficialCompressionProfile,
        ParameterOverrides,
    };

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
        let raw = descriptor["modelExperiments"]["experiments"]
            ["CASCADE_USE_EXPERIMENT_CHECKPOINTER"]["stringValue"]
            .as_str()
            .expect("custom model must contain checkpoint settings");
        serde_json::from_str(raw).expect("checkpoint settings must be valid JSON")
    }

    #[test]
    fn uses_experience_defaults_when_limits_are_missing() {
        let (virtual_model, upstream_model) = models();

        let descriptor =
            AntigravityModelDescriptor::build_model_object(&virtual_model, &upstream_model);
        let catalog = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
            &virtual_model,
            &upstream_model,
        );

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
        let catalog = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
            &virtual_model,
            &upstream_model,
        );

        assert_eq!(descriptor["contextWindow"], 1_000_000);
        assert_eq!(descriptor["inputTokenLimit"], 1_000_000);
        assert_eq!(descriptor["outputTokenLimit"], 65_536);
        assert_eq!(catalog["contextWindow"], 1_000_000);
        assert_eq!(catalog["maxTokens"], 1_000_000);
        assert_eq!(catalog["maxOutputTokens"], 65_536);
    }

    #[test]
    fn applies_percentage_to_custom_model_checkpoint_threshold() {
        let settings = OfficialModelSettings {
            custom_model_threshold_percent: Some(80),
            ..OfficialModelSettings::default()
        };

        assert_eq!(
            settings.custom_model_checkpoint_limits(372_000, 128_000),
            Some((297_600, 372_000, 16_384))
        );
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
        let settings = OfficialModelSettings {
            custom_model_threshold_percent: Some(80),
            ..OfficialModelSettings::default()
        };

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
            assert_eq!(checkpoint["token_threshold"], "297600");
            assert_eq!(checkpoint["max_token_limit"], "372000");
            assert_eq!(checkpoint["max_output_tokens"], "16384");
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
            gemini_compression_profile: OfficialCompressionProfile::Custom,
            custom_model_threshold_percent: Some(60),
            gemini_token_threshold: 150_000,
            gemini_max_token_limit: 320_000,
            gemini_max_output_tokens: 30_000,
        };
        let mut catalog = json!({ "models": {} });

        AntigravityModelDescriptor::inject_into_model_list_with_settings(
            &mut catalog,
            &[first_virtual_model, second_virtual_model],
            &[first_upstream_model, second_upstream_model],
            &settings,
        );

        let first_checkpoint = checkpoint(&catalog["models"]["custom-model"]);
        assert_eq!(first_checkpoint["token_threshold"], "256000");
        assert_eq!(first_checkpoint["max_token_limit"], "320000");
        assert_eq!(first_checkpoint["max_output_tokens"], "30000");

        let second_checkpoint = checkpoint(&catalog["models"]["custom-model-2"]);
        assert_eq!(second_checkpoint["token_threshold"], "192000");
        assert_eq!(second_checkpoint["max_token_limit"], "320000");
        assert_eq!(second_checkpoint["max_output_tokens"], "30000");
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
        let settings = OfficialModelSettings {
            gemini_compression_profile: OfficialCompressionProfile::Aggressive,
            custom_model_threshold_percent: Some(95),
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
        assert_eq!(checkpoint["token_threshold"], "190000");
        assert_eq!(checkpoint["max_token_limit"], "200000");
        assert_eq!(checkpoint["max_output_tokens"], "10000");
    }

    #[test]
    fn custom_checkpoint_hard_limit_does_not_exceed_context_window() {
        let (virtual_model, mut upstream_model) = models();
        upstream_model.token_limits = ModelTokenLimits {
            context_window: Some(200_000),
            input_token_limit: Some(372_000),
            output_token_limit: Some(128_000),
            ..ModelTokenLimits::default()
        };
        let settings = OfficialModelSettings {
            custom_model_threshold_percent: Some(80),
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
        assert_eq!(checkpoint["token_threshold"], "160000");
        assert_eq!(checkpoint["max_token_limit"], "200000");
        assert_eq!(checkpoint["max_output_tokens"], "16384");
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
            gemini_compression_profile: crate::domain::OfficialCompressionProfile::Balanced,
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
}

fn supported_mime_types(vision: bool) -> Map<String, Value> {
    let mut mime_types = Map::from_iter([
        ("text/plain".to_string(), Value::Bool(true)),
        ("text/markdown".to_string(), Value::Bool(true)),
        ("application/json".to_string(), Value::Bool(true)),
    ]);
    if vision {
        for mime_type in ["image/png", "image/jpeg", "image/webp"] {
            mime_types.insert(mime_type.to_string(), Value::Bool(true));
        }
    }
    mime_types
}
