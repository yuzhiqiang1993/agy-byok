use super::checkpoint::{
    apply_model_compression_policy, canonical_model_id, official_checkpoint_output_limits,
    official_model_aliases,
};
use super::{catalog_container_mut, AntigravityModelDescriptor};
use crate::domain::{
    ModelModality, ModelRole, Provider, ProviderProtocol, ReasoningMapping, UpstreamModel,
    VirtualModel,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

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
        provider: &Provider,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let host_model_id = virtual_model.effective_host_model_id().into_owned();
        let (_, input_token_limit, output_token_limit) = token_limits(upstream_model);

        let mut descriptor = json!({
            "displayName": virtual_model.display_name,
            // Antigravity 的 maxTokens 是 planner 输入预算，不是请求的输出参数。
            "maxTokens": input_token_limit,
            "maxOutputTokens": output_token_limit,
            "model": host_model_id,
            "planModel": host_model_id,
            "requestedModel": host_model_id,
            // 宿主仍通过 Gemini 传输链路调用 BYOK，模型归属则按上游协议单独声明。
            "apiProvider": "API_PROVIDER_GOOGLE_GEMINI",
            "modelProvider": host_model_provider(&provider.protocol),
            // 自定义模型不冒充宿主官方推荐项。
            "recommended": false,
            "supportsImages": caps.supports_input(ModelModality::Image),
            "supportsThinking": caps.reasoning.supports_reasoning(),
            "supportsVideo": caps.supports_input(ModelModality::Video),
            "supportedMimeTypes": input_mime_types(caps)
        });
        let provider_name = provider.name.trim();
        if !provider_name.is_empty() {
            descriptor["tagTitle"] = Value::String(provider_name.to_string());
            descriptor["tagDescription"] = Value::String(format!("Provider: {}", provider_name));
        }
        apply_reasoning_metadata(&mut descriptor, virtual_model, upstream_model);
        descriptor
    }

    pub fn inject_into_model_list(
        models_json: &mut Value,
        virtual_models: &[VirtualModel],
        upstream_models: &[UpstreamModel],
        providers: &[Provider],
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
                    .and_then(|upstream_model| {
                        providers
                            .iter()
                            .find(|provider| {
                                provider.id == upstream_model.provider_id && provider.enabled
                            })
                            .map(|provider| (virtual_model, upstream_model, provider))
                    })
            })
            .collect::<Vec<_>>();

        let aliases = official_model_aliases(models_json);
        let checkpoint_output_limits = official_checkpoint_output_limits(models_json);

        let catalog = catalog_container_mut(models_json);
        if catalog.get("models").is_some() {
            let model_role_ids = {
                let target = catalog
                    .get("models")
                    .expect("checked model catalog must exist");
                models
                    .iter()
                    .map(|(virtual_model, upstream_model, _)| {
                        let catalog_id = if target.is_array() {
                            virtual_model.id.clone()
                        } else {
                            virtual_model.catalog_key().into_owned()
                        };
                        (
                            catalog_id,
                            upstream_model
                                .capabilities
                                .roles
                                .contains(&ModelRole::Agent),
                            upstream_model
                                .capabilities
                                .roles
                                .contains(&ModelRole::ImageGeneration),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let all_catalog_ids = model_role_ids
                .iter()
                .map(|(model_id, _, _)| model_id.clone())
                .collect::<Vec<_>>();
            let agent_model_ids = model_role_ids
                .iter()
                .filter(|(_, is_agent, _)| *is_agent)
                .map(|(model_id, _, _)| model_id.clone())
                .collect::<Vec<_>>();
            let image_generation_model_ids = model_role_ids
                .iter()
                .filter(|(_, _, is_image_generation)| *is_image_generation)
                .map(|(model_id, _, _)| model_id.clone())
                .collect::<Vec<_>>();
            // 为启用推理的自定义模型生成 tiered 母条目并注册 tieredModelIds，
            // 供新版 Antigravity 模型选择器按「单模型 + 档位子菜单」聚类。
            inject_tiered_catalog(catalog, &models);
            inject_models(
                catalog
                    .get_mut("models")
                    .expect("checked model catalog must exist"),
                models,
                &checkpoint_output_limits,
                &aliases,
            );
            place_catalog_keys_in_byok_sort(
                catalog.get_mut("agentModelSorts"),
                &all_catalog_ids,
                &agent_model_ids,
            );
            place_catalog_keys_in_role_list(
                catalog,
                "imageGenerationModelIds",
                &all_catalog_ids,
                &image_generation_model_ids,
            );
        } else {
            inject_models(catalog, models, &checkpoint_output_limits, &aliases);
        }
    }
}

/// 按上游模型聚合档位条目，为每个启用推理的自定义模型生成一个
/// `-tiered` 母条目并注册到 `tieredModelIds`，供 Antigravity 新版
/// 模型选择器按「单模型 + 档位子菜单」聚类显示。
fn inject_tiered_catalog(
    catalog: &mut Value,
    models: &[(&VirtualModel, &UpstreamModel, &Provider)],
) {
    // 同一上游模型只生成一个母条目（取第一个档位条目作为模板）。
    let mut groups: Vec<(&VirtualModel, &UpstreamModel, &Provider)> = Vec::new();
    let mut seen_upstreams: HashSet<&str> = HashSet::new();
    for (virtual_model, upstream_model, provider) in models {
        if !upstream_model.capabilities.reasoning.supports_reasoning() {
            continue;
        }
        if seen_upstreams.insert(upstream_model.id.as_str()) {
            groups.push((virtual_model, upstream_model, provider));
        }
    }
    if groups.is_empty() {
        return;
    }

    // 先构建母条目，避免在持有 models 可变借用时迭代 groups。
    let mut entries_to_insert: Vec<(String, Value)> = Vec::new();
    for (virtual_model, upstream_model, provider) in &groups {
        let catalog_key = virtual_model.catalog_key();
        let base_id = strip_level_suffix(catalog_key.as_ref());
        let tiered_key = format!("{base_id}-tiered");
        let base_display_name = strip_display_level_suffix(&virtual_model.display_name);
        let mut entry = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
            virtual_model,
            upstream_model,
            provider,
        );
        entry["displayName"] = json!(base_display_name);
        // 母条目表示「动态档位」，由 IDE 侧子菜单选择后以具体档位条目发请求。
        entry["thinkingBudget"] = json!(-1);
        entries_to_insert.push((tiered_key, entry));
    }
    if entries_to_insert.is_empty() {
        return;
    }

    let Some(models_obj) = catalog.get_mut("models").and_then(Value::as_object_mut) else {
        return;
    };
    let mut tiered_entries: Vec<String> = Vec::new();
    for (tiered_key, entry) in entries_to_insert {
        if !models_obj.contains_key(&tiered_key) {
            models_obj.insert(tiered_key.clone(), entry);
            tiered_entries.push(tiered_key);
        }
    }
    if tiered_entries.is_empty() {
        return;
    }

    // 注册到 tieredModelIds。分组 key 先使用实验值 "custom"，
    // 待实测确认 Antigravity 对非官方分组 key 的处理后再调整。
    let group_key = "custom";
    let mut tiered_model_ids = catalog
        .get_mut("tieredModelIds")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let existing = tiered_model_ids
        .get_mut(group_key)
        .and_then(Value::as_array_mut);
    if let Some(array) = existing {
        for key in &tiered_entries {
            if !array.iter().any(|item| item.as_str() == Some(key)) {
                array.push(json!(key));
            }
        }
    } else {
        tiered_model_ids[group_key] = json!(tiered_entries);
    }
    catalog["tieredModelIds"] = tiered_model_ids;
}

/// 去掉 ID 末尾的档位后缀（长后缀优先，避免 `-x-high` 被 `-high` 误拆）。
fn strip_level_suffix(id: &str) -> &str {
    const LEVELS: &[&str] = &[
        "adaptive", "x-high", "medium", "auto", "high", "max", "low", "off",
    ];
    for level in LEVELS {
        let suffix = format!("-{level}");
        if let Some(base) = id.strip_suffix(&suffix) {
            return base;
        }
    }
    id
}

/// 去掉 displayName 末尾的档位后缀（如 ` (High)` / ` (X-High)` / ` high`）。
fn strip_display_level_suffix(display_name: &str) -> String {
    const LEVELS: &[&str] = &[
        "adaptive", "x-high", "medium", "auto", "custom", "default", "high", "max", "low", "off",
    ];
    let result = display_name.trim().to_string();
    for level in LEVELS {
        for suffix in [format!(" ({level})"), format!(" {level}")] {
            if let Some(stripped) = strip_case_insensitive_suffix(&result, &suffix) {
                return stripped.trim_end().to_string();
            }
        }
    }
    result
}

fn strip_case_insensitive_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    value
        .len()
        .checked_sub(suffix.len())
        .filter(|start| value[*start..].eq_ignore_ascii_case(suffix))
        .map(|start| &value[..start])
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
    descriptor.insert(
        "thinkingBudget".to_string(),
        json!(reasoning.thinking_budget.unwrap_or(-1)),
    );
    if let Some(tokens) = reasoning.min_thinking_budget {
        descriptor.insert("minThinkingBudget".to_string(), json!(tokens));
    }
    let Some(level) = virtual_model.default_reasoning_level else {
        return;
    };
    let Some(mapping) = upstream_model.capabilities.reasoning.mapping_for(level) else {
        return;
    };

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
    provider: &Provider,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) -> Value {
    let mut descriptor = AntigravityModelDescriptor::build_cloud_code_catalog_entry(
        virtual_model,
        upstream_model,
        provider,
    );
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
    models: Vec<(&VirtualModel, &UpstreamModel, &Provider)>,
    checkpoint_output_limits: &std::collections::BTreeMap<String, u32>,
    aliases: &std::collections::BTreeMap<String, String>,
) {
    match target {
        Value::Array(entries) => {
            entries.extend(
                models
                    .into_iter()
                    .map(|(virtual_model, upstream_model, _)| {
                        custom_model_object(
                            virtual_model,
                            upstream_model,
                            checkpoint_output_limits,
                            aliases,
                        )
                    }),
            );
        }
        Value::Object(entries) => {
            for (virtual_model, upstream_model, provider) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
                        provider,
                        checkpoint_output_limits,
                        aliases,
                    ),
                );
            }
        }
        _ => {
            let mut entries = Map::new();
            for (virtual_model, upstream_model, provider) in models {
                entries.insert(
                    virtual_model.catalog_key().into_owned(),
                    custom_cloud_code_catalog_entry(
                        virtual_model,
                        upstream_model,
                        provider,
                        checkpoint_output_limits,
                        aliases,
                    ),
                );
            }
            *target = Value::Object(entries);
        }
    }
}

fn place_catalog_keys_in_byok_sort(
    model_sorts: Option<&mut Value>,
    all_catalog_keys: &[String],
    agent_catalog_keys: &[String],
) {
    if all_catalog_keys.is_empty() {
        return;
    }
    let Some(model_sorts) = model_sorts.and_then(Value::as_array_mut) else {
        return;
    };

    let non_agent_catalog_keys = all_catalog_keys
        .iter()
        .filter(|key| !agent_catalog_keys.contains(key))
        .collect::<Vec<_>>();

    for model_sort in model_sorts.iter_mut() {
        let Some(groups) = model_sort.get_mut("groups").and_then(Value::as_array_mut) else {
            continue;
        };

        for group in groups {
            let Some(model_ids) = group.get_mut("modelIds").and_then(Value::as_array_mut) else {
                continue;
            };

            if !non_agent_catalog_keys.is_empty() {
                model_ids.retain(|model_id| {
                    model_id.as_str().is_none_or(|id| {
                        !non_agent_catalog_keys
                            .iter()
                            .any(|non_agent_key| *non_agent_key == id)
                    })
                });
            }

            for catalog_key in agent_catalog_keys {
                if !model_ids
                    .iter()
                    .any(|model_id| model_id.as_str() == Some(catalog_key.as_str()))
                {
                    model_ids.push(Value::String(catalog_key.clone()));
                }
            }
        }
    }

    if agent_catalog_keys.is_empty() {
        return;
    }

    let byok_sort_index = model_sorts
        .iter()
        .position(|sort| sort.get("displayName").and_then(Value::as_str) == Some("BYOK"));
    let byok_sort = match byok_sort_index {
        Some(index) => &mut model_sorts[index],
        None => {
            model_sorts.push(json!({
                "displayName": "BYOK",
                "groups": [{ "modelIds": [] }]
            }));
            model_sorts
                .last_mut()
                .expect("BYOK model sort was just appended")
        }
    };
    if !byok_sort.get("groups").is_some_and(Value::is_array) {
        byok_sort["groups"] = json!([]);
    }
    let groups = byok_sort
        .get_mut("groups")
        .and_then(Value::as_array_mut)
        .expect("BYOK groups were normalized to an array");
    if groups.is_empty() {
        groups.push(json!({ "modelIds": [] }));
    }
    if !groups[0].get("modelIds").is_some_and(Value::is_array) {
        groups[0]["modelIds"] = json!([]);
    }
    let model_ids = groups[0]
        .get_mut("modelIds")
        .and_then(Value::as_array_mut)
        .expect("BYOK model IDs were normalized to an array");
    for catalog_key in agent_catalog_keys {
        if !model_ids
            .iter()
            .any(|model_id| model_id.as_str() == Some(catalog_key.as_str()))
        {
            model_ids.push(Value::String(catalog_key.clone()));
        }
    }
}

fn place_catalog_keys_in_role_list(
    catalog: &mut Value,
    field: &str,
    _all_catalog_keys: &[String],
    selected_catalog_keys: &[String],
) {
    if selected_catalog_keys.is_empty() {
        return;
    }
    let Some(catalog) = catalog.as_object_mut() else {
        return;
    };
    if !catalog.get(field).is_some_and(Value::is_array) {
        catalog.insert(field.to_string(), json!([]));
    }
    let model_ids = catalog
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .expect("role model IDs were normalized to an array");
    for catalog_key in selected_catalog_keys {
        if !model_ids
            .iter()
            .any(|model_id| model_id.as_str() == Some(catalog_key.as_str()))
        {
            model_ids.push(Value::String(catalog_key.clone()));
        }
    }
}

fn host_model_provider(protocol: &ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
            "MODEL_PROVIDER_OPENAI"
        }
        ProviderProtocol::AnthropicMessages => "MODEL_PROVIDER_ANTHROPIC",
        ProviderProtocol::GeminiGenerateContent => "MODEL_PROVIDER_GOOGLE",
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
    let mut mime_types = Map::new();
    for mime_type in caps.effective_input_mime_types() {
        mime_types.insert(mime_type, Value::Bool(true));
    }
    mime_types
}
