use super::AntigravityModelDescriptor;
use crate::domain::{OfficialModelSettings, ReasoningMapping, UpstreamModel, VirtualModel};
use serde_json::{json, Map, Value};

// 供应商目录没有提供限制时使用保守的经验回退值；它不会写回模型配置。
// 只要目录返回了模型级限制，token_limits() 就会优先使用真实值。
pub(super) const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
pub(super) const DEFAULT_INPUT_TOKEN_LIMIT: u32 = 128_000;
pub(super) const DEFAULT_OUTPUT_TOKEN_LIMIT: u32 = 128_000;

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

    #[cfg(test)]
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

    /// 自定义占位模型只注入路由和能力元数据，不注入实验性 Checkpointer。
    /// Language Server 会把自指的 BYOM `checkpoint_model` 当成有效状态源，
    /// 但该链路无法产出可恢复的 Checkpoint，进而在正常生成前直接失败。
    pub fn inject_into_model_list_with_settings(
        models_json: &mut Value,
        virtual_models: &[VirtualModel],
        upstream_models: &[UpstreamModel],
        _settings: &OfficialModelSettings,
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
                inject_models(target, models);
                if let Some(catalog_keys) = catalog_keys {
                    append_catalog_keys_to_model_sorts(
                        catalog.get_mut("agentModelSorts"),
                        &catalog_keys,
                    );
                }
                return;
            }
        }

        inject_models(models_json, models);
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

fn custom_model_object(virtual_model: &VirtualModel, upstream_model: &UpstreamModel) -> Value {
    AntigravityModelDescriptor::build_model_object(virtual_model, upstream_model)
}

fn custom_cloud_code_catalog_entry(
    virtual_model: &VirtualModel,
    upstream_model: &UpstreamModel,
) -> Value {
    AntigravityModelDescriptor::build_cloud_code_catalog_entry(virtual_model, upstream_model)
}

fn inject_models(target: &mut Value, models: Vec<(&VirtualModel, &UpstreamModel)>) {
    match target {
        Value::Array(entries) => {
            entries.extend(models.into_iter().map(|(virtual_model, upstream_model)| {
                custom_model_object(virtual_model, upstream_model)
            }));
        }
        Value::Object(entries) => {
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(virtual_model, upstream_model),
                );
            }
        }
        _ => {
            let mut entries = Map::new();
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(virtual_model, upstream_model),
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
