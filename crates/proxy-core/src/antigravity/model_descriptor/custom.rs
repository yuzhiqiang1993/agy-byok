use super::checkpoint::{
    apply_model_compression_policy, canonical_model_id, official_checkpoint_output_limits,
    official_model_aliases,
};
use super::{catalog_container_mut, AntigravityModelDescriptor};
use crate::domain::{ModelModality, ReasoningMapping, UpstreamModel, VirtualModel};
use serde_json::{json, Map, Value};

// 供应商目录没有提供限制时使用保守的经验回退值；它不会写回模型配置。
// 只要目录返回了模型级限制，token_limits() 就会优先使用真实值。
pub(super) const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
pub(super) const DEFAULT_INPUT_TOKEN_LIMIT: u32 = 128_000;
pub(super) const DEFAULT_OUTPUT_TOKEN_LIMIT: u32 = 65_536;

impl AntigravityModelDescriptor {
    pub fn build_model_object(
        virtual_model: &VirtualModel,
        upstream_model: &UpstreamModel,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let host_model_id = virtual_model.effective_host_model_id().into_owned();
        let input_mime_types = input_mime_types(caps);
        let declared_input_modalities = modalities(&caps.input_modalities, false);
        let declared_output_modalities = modalities(&caps.output_modalities, false);
        let (context_window, input_token_limit, output_token_limit) = token_limits(upstream_model);

        let mut descriptor = json!({
            "id": virtual_model.id,
            "name": format!("models/{host_model_id}"),
            "displayName": virtual_model.display_name,
            "description": format!("Custom BYOK Model (Provider: {})", upstream_model.provider_id),
            "contextWindow": context_window,
            "inputTokenLimit": input_token_limit,
            "outputTokenLimit": output_token_limit,
            "supportsImages": caps.supports_input(ModelModality::Image),
            "supportsAudio": caps.supports_input(ModelModality::Audio),
            "supportsVideo": caps.supports_input(ModelModality::Video),
            "supportsTools": caps.tools,
            "supportsThinking": caps.reasoning.supports_reasoning(),
            "inputModalities": declared_input_modalities,
            "outputModalities": declared_output_modalities,
            "supportedMimeTypes": input_mime_types.keys().collect::<Vec<_>>()
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
        let declared_input_modalities = modalities(&caps.input_modalities, false);
        let input_modalities_lowercase = modalities(&caps.input_modalities, true);
        let declared_output_modalities = modalities(&caps.output_modalities, false);
        let output_modalities_lowercase = modalities(&caps.output_modalities, true);
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
            // 自定义模型不冒充宿主官方推荐项。
            "recommended": false,
            "supportsImages": caps.supports_input(ModelModality::Image),
            "supportsVision": caps.supports_input(ModelModality::Image),
            "supportsImage": caps.supports_input(ModelModality::Image),
            "supportsThinking": caps.reasoning.supports_reasoning(),
            "supportsAudio": caps.supports_input(ModelModality::Audio),
            "supportsVideo": caps.supports_input(ModelModality::Video),
            "inputModalities": declared_input_modalities,
            "input_modalities": input_modalities_lowercase,
            "outputModalities": declared_output_modalities,
            "output_modalities": output_modalities_lowercase,
            "supportedMimeTypes": input_mime_types(caps),
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

        let aliases = official_model_aliases(models_json);
        let checkpoint_output_limits = official_checkpoint_output_limits(models_json);

        let catalog = catalog_container_mut(models_json);
        if catalog.get("models").is_some() {
            let model_sort_ids = {
                let target = catalog
                    .get("models")
                    .expect("checked model catalog must exist");
                models
                    .iter()
                    .map(|(virtual_model, _)| {
                        if target.is_array() {
                            virtual_model.id.clone()
                        } else {
                            virtual_model.catalog_key().into_owned()
                        }
                    })
                    .collect::<Vec<_>>()
            };
            inject_models(
                catalog
                    .get_mut("models")
                    .expect("checked model catalog must exist"),
                models,
                &checkpoint_output_limits,
                &aliases,
            );
            append_catalog_keys_to_model_sorts(catalog.get_mut("agentModelSorts"), &model_sort_ids);
        } else {
            inject_models(catalog, models, &checkpoint_output_limits, &aliases);
        }
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
    let reasoning = &upstream_model.capabilities.reasoning;
    if let Some(tokens) = reasoning.thinking_budget {
        descriptor.insert("thinkingBudget".to_string(), json!(tokens));
    }
    if let Some(tokens) = reasoning.min_thinking_budget {
        descriptor.insert("minThinkingBudget".to_string(), json!(tokens));
    }
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

fn apply_custom_model_compression_policy(
    descriptor: &mut Value,
    upstream_model: &UpstreamModel,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) {
    let Some(policy) = upstream_model.compression_policy.as_ref() else {
        return;
    };
    if !policy.enabled {
        return;
    }
    let (_, _, output_token_limit) = token_limits(upstream_model);
    let capacity = upstream_model
        .token_limits
        .effective_compression_capacity(DEFAULT_CONTEXT_WINDOW, DEFAULT_INPUT_TOKEN_LIMIT);
    let canonical_checkpoint = canonical_model_id(&policy.checkpoint_model, aliases);
    let effective_output_limit = checkpoint_output_limits
        .get(canonical_checkpoint)
        .or_else(|| checkpoint_output_limits.get(&policy.checkpoint_model))
        .copied()
        .map_or(output_token_limit, |checkpoint_limit| {
            output_token_limit.min(checkpoint_limit)
        });
    apply_model_compression_policy(
        descriptor,
        policy,
        capacity,
        effective_output_limit,
        Some(policy),
    );
}

fn custom_model_object(
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) -> Value {
    let mut descriptor =
        AntigravityModelDescriptor::build_model_object(virtual_model, upstream_model);
    apply_custom_model_compression_policy(
        &mut descriptor,
        upstream_model,
        checkpoint_output_limits,
        aliases,
    );
    descriptor
}

fn custom_cloud_code_catalog_entry(
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) -> Value {
    let mut descriptor =
        AntigravityModelDescriptor::build_cloud_code_catalog_entry(virtual_model, upstream_model);
    apply_custom_model_compression_policy(
        &mut descriptor,
        upstream_model,
        checkpoint_output_limits,
        aliases,
    );
    descriptor
}

fn inject_models(
    target: &mut Value,
    models: Vec<(&VirtualModel, &UpstreamModel)>,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) {
    match target {
        Value::Array(entries) => {
            entries.extend(models.into_iter().map(|(virtual_model, upstream_model)| {
                custom_model_object(
                    virtual_model,
                    upstream_model,
                    checkpoint_output_limits,
                    aliases,
                )
            }));
        }
        Value::Object(entries) => {
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
                        checkpoint_output_limits,
                        aliases,
                    ),
                );
            }
        }
        _ => {
            let mut entries = Map::new();
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
                        checkpoint_output_limits,
                        aliases,
                    ),
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

fn modalities(
    modalities: &std::collections::BTreeSet<ModelModality>,
    lowercase: bool,
) -> Vec<&'static str> {
    modalities
        .iter()
        .map(|modality| match (modality, lowercase) {
            (ModelModality::Text, false) => "TEXT",
            (ModelModality::Image, false) => "IMAGE",
            (ModelModality::Audio, false) => "AUDIO",
            (ModelModality::Video, false) => "VIDEO",
            (ModelModality::Document, false) => "DOCUMENT",
            (ModelModality::Text, true) => "text",
            (ModelModality::Image, true) => "image",
            (ModelModality::Audio, true) => "audio",
            (ModelModality::Video, true) => "video",
            (ModelModality::Document, true) => "document",
        })
        .collect()
}

fn input_mime_types(caps: &crate::domain::ModelCapabilities) -> Map<String, Value> {
    let mut mime_types = Map::from_iter([
        ("text/plain".to_string(), Value::Bool(true)),
        ("text/markdown".to_string(), Value::Bool(true)),
        ("application/json".to_string(), Value::Bool(true)),
    ]);
    for mime_type in caps.effective_input_mime_types() {
        mime_types.insert(mime_type, Value::Bool(true));
    }
    mime_types
}
