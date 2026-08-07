use crate::domain::model::ClaudeCheckpointMetadata;
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

    /// 覆盖官方 Gemini 与 Claude 模型目录中的检查点参数。
    ///
    /// Antigravity IDE 会从 `modelExperiments` 中读取检查点策略；这与实际
    /// 生成请求的 `max_tokens` 不是同一层配置。官方档位不做任何改写，避免
    /// 上游将来的参数变化被本地默认值遮蔽。
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        settings: &OfficialModelSettings,
    ) {
        if models_json.get("models").is_some() {
            if let Some(models) = models_json.get_mut("models") {
                apply_checkpoint_overrides_to_models(models, settings);
            }
        } else {
            apply_checkpoint_overrides_to_models(models_json, settings);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficialModelFamily {
    Gemini,
    Claude,
}

fn apply_checkpoint_overrides_to_models(models: &mut Value, settings: &OfficialModelSettings) {
    match models {
        Value::Object(entries) => {
            for (key, entry) in entries {
                apply_official_checkpoint_override(Some(key), entry, settings);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                apply_official_checkpoint_override(None, entry, settings);
            }
        }
        _ => {}
    }
}

fn apply_official_checkpoint_override(
    key: Option<&str>,
    entry: &mut Value,
    settings: &OfficialModelSettings,
) {
    let Some(family) = official_model_family(key, entry) else {
        return;
    };
    let limits = match family {
        OfficialModelFamily::Gemini => settings.gemini_checkpoint_limits(),
        OfficialModelFamily::Claude => claude_checkpoint_metadata(entry)
            .and_then(|metadata| settings.claude_checkpoint_limits(metadata)),
    };
    let Some((threshold, max_token_limit, max_output_tokens)) = limits else {
        return;
    };
    let fallback_checkpoint_model = match family {
        OfficialModelFamily::Gemini => "MODEL_GEMINI",
        OfficialModelFamily::Claude => key.unwrap_or("MODEL_CLAUDE"),
    };
    apply_checkpoint_override(
        entry,
        threshold,
        max_token_limit,
        max_output_tokens,
        fallback_checkpoint_model,
    );
}

fn official_model_family(key: Option<&str>, entry: &Value) -> Option<OfficialModelFamily> {
    let mut candidates = Vec::with_capacity(8);
    if let Some(key) = key {
        candidates.push(key);
    }
    for field in [
        "id",
        "model",
        "modelId",
        "requestedModel",
        "planModel",
        "displayName",
        "name",
    ] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            candidates.push(value);
        }
    }
    if candidates
        .iter()
        .any(|value| is_custom_model_identity(value))
    {
        return None;
    }

    let mut is_gemini = false;
    let mut is_claude = false;
    for value in candidates {
        mark_model_family(value, &mut is_gemini, &mut is_claude);
    }
    if let Some(checkpoint_model) = existing_checkpoint(entry)
        .as_ref()
        .and_then(|checkpoint| checkpoint.get("checkpoint_model"))
        .and_then(Value::as_str)
    {
        mark_model_family(checkpoint_model, &mut is_gemini, &mut is_claude);
    }
    for field in ["modelProvider", "apiProvider"] {
        if entry
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| value.to_ascii_lowercase().contains("anthropic"))
        {
            is_claude = true;
        }
    }

    match (is_gemini, is_claude) {
        (true, false) => Some(OfficialModelFamily::Gemini),
        (false, true) => Some(OfficialModelFamily::Claude),
        _ => None,
    }
}

fn mark_model_family(value: &str, is_gemini: &mut bool, is_claude: &mut bool) {
    let normalized = value.to_ascii_lowercase();
    *is_gemini |= normalized.contains("gemini");
    *is_claude |= normalized.contains("claude");
}

fn is_custom_model_identity(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    if normalized.starts_with("custom-") {
        return true;
    }
    let model_id = normalized.strip_prefix("models/").unwrap_or(&normalized);
    model_id
        .strip_prefix("model_placeholder_m")
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (400..600).contains(&value))
}

fn claude_checkpoint_metadata(entry: &Value) -> Option<ClaudeCheckpointMetadata> {
    let capacity =
        minimum_positive_field(entry, &["maxTokens", "inputTokenLimit", "contextWindow"])?;
    let output_token_limit =
        minimum_positive_field(entry, &["maxOutputTokens", "outputTokenLimit"]);

    Some(ClaudeCheckpointMetadata {
        capacity,
        output_token_limit,
    })
}

fn existing_checkpoint(entry: &Value) -> Option<Value> {
    entry
        .get("modelExperiments")?
        .get("experiments")?
        .get("CASCADE_USE_EXPERIMENT_CHECKPOINTER")?
        .get("stringValue")?
        .as_str()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
}

fn minimum_positive_field(entry: &Value, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .filter_map(|field| entry.get(*field).and_then(positive_u32))
        .min()
}

fn positive_u32(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.parse::<u32>().ok()))
        .filter(|value| *value > 0)
}

fn apply_checkpoint_override(
    entry: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
    fallback_checkpoint_model: &str,
) {
    let checkpoint_model = existing_checkpoint(entry)
        .as_ref()
        .and_then(|checkpoint| checkpoint.get("checkpoint_model"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            ["model", "modelId", "requestedModel", "planModel", "id"]
                .iter()
                .find_map(|field| entry.get(*field).and_then(Value::as_str))
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback_checkpoint_model.to_string());
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
        ClaudeCompressionProfile, CustomModelCompressionProfile, ModelCapabilities,
        ModelCheckpointOverride, ModelTokenLimits, OfficialCompressionProfile, ParameterOverrides,
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
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 60,
            custom_model_max_token_limit_percent: 80,
            custom_model_max_output_tokens_percent: 5,
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
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 70,
            custom_model_max_token_limit_percent: 90,
            custom_model_max_output_tokens_percent: 5,
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
            custom_model_compression_profile: CustomModelCompressionProfile::Balanced,
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
            claude_compression_profile: ClaudeCompressionProfile::Safe,
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
            (ClaudeCompressionProfile::Safe, (82_015, 97_656, 3_125)),
            (
                ClaudeCompressionProfile::Balanced,
                (122_070, 146_484, 3_125),
            ),
            (
                ClaudeCompressionProfile::Aggressive,
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
                claude_compression_profile: profile,
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
            claude_compression_profile: ClaudeCompressionProfile::Custom,
            claude_token_threshold_percent: 70,
            claude_max_token_limit_percent: 90,
            claude_max_output_tokens_percent: 5,
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
            gemini_compression_profile: OfficialCompressionProfile::Safe,
            claude_compression_profile: ClaudeCompressionProfile::Safe,
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
            gemini_compression_profile: OfficialCompressionProfile::Safe,
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
            gemini_compression_profile: OfficialCompressionProfile::Safe,
            claude_compression_profile: ClaudeCompressionProfile::Balanced,
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 40,
            custom_model_max_token_limit_percent: 60,
            custom_model_max_output_tokens_percent: 5,
            ..OfficialModelSettings::default()
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
            claude_compression_profile: ClaudeCompressionProfile::Safe,
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
            claude_compression_profile: ClaudeCompressionProfile::Safe,
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
