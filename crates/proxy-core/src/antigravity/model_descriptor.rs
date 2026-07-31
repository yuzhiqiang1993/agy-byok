use crate::domain::{ReasoningMapping, UpstreamModel, VirtualModel};
use serde_json::{json, Map, Value};

pub struct AntigravityModelDescriptor;

impl AntigravityModelDescriptor {
    pub fn build_model_object(
        virtual_model: &VirtualModel,
        upstream_model: &UpstreamModel,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let host_model_id = virtual_model.effective_host_model_id().into_owned();
        let supported_mime_types = supported_mime_types(caps.vision);

        let mut descriptor = json!({
            "id": virtual_model.id,
            "name": format!("models/{host_model_id}"),
            "displayName": virtual_model.display_name,
            "description": format!("Custom BYOK Model (Provider: {})", upstream_model.provider_id),
            "inputTokenLimit": 128000,
            "outputTokenLimit": 8192,
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

        let mut descriptor = json!({
            "displayName": virtual_model.display_name,
            "maxTokens": 128000,
            "maxOutputTokens": 8192,
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
        descriptor.insert(
            "reasoningEffort".to_string(),
            Value::String("auto".to_string()),
        );
        if upstream_model
            .capabilities
            .reasoning
            .levels
            .values()
            .any(|mapping| {
                matches!(
                    mapping,
                    ReasoningMapping::BudgetTokens(_)
                        | ReasoningMapping::Adaptive
                        | ReasoningMapping::Disabled
                )
            })
        {
            descriptor.insert(
                "thinkingBudget".to_string(),
                Value::String("auto".to_string()),
            );
        }
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
        ReasoningMapping::BudgetTokens(_) => {
            descriptor.insert(
                "thinkingBudget".to_string(),
                Value::String("enabled".to_string()),
            );
        }
        ReasoningMapping::Adaptive => {
            descriptor.insert(
                "thinkingBudget".to_string(),
                Value::String("auto".to_string()),
            );
        }
        ReasoningMapping::Disabled => {
            descriptor.insert(
                "thinkingBudget".to_string(),
                Value::String("disabled".to_string()),
            );
        }
        ReasoningMapping::Effort(_) | ReasoningMapping::NativeLevel(_) => {}
    }
}

fn inject_models(target: &mut Value, models: Vec<(&VirtualModel, &UpstreamModel)>) {
    match target {
        Value::Array(entries) => {
            entries.extend(models.into_iter().map(|(virtual_model, upstream_model)| {
                AntigravityModelDescriptor::build_model_object(virtual_model, upstream_model)
            }));
        }
        Value::Object(entries) => {
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    AntigravityModelDescriptor::build_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
                    ),
                );
            }
        }
        _ => {
            let mut entries = Map::new();
            for (virtual_model, upstream_model) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    AntigravityModelDescriptor::build_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
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
