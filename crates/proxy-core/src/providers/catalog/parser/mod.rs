mod reasoning;

use super::{ProviderCatalogModel, UpstreamCompressionPolicy};
use crate::domain::{
    CustomModelCheckpointRetryConfig, ModelCompressionPolicy, ModelModality, ModelRole,
    ProviderProtocol, MODEL_NAMESPACE_PREFIX,
};
use reasoning::parse_reasoning_metadata;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

struct CatalogModalityMetadata {
    input_mime_types: Option<Vec<String>>,
    input_modalities: Option<BTreeSet<ModelModality>>,
    output_modalities: Option<BTreeSet<ModelModality>>,
}

#[cfg(test)]
pub(super) fn parse_catalog_models(
    payload: &Value,
    protocol: &ProviderProtocol,
) -> Vec<ProviderCatalogModel> {
    parse_catalog_models_with_context(payload, protocol, false)
}

pub(super) fn parse_catalog_models_with_context(
    payload: &Value,
    protocol: &ProviderProtocol,
    is_cpa_catalog: bool,
) -> Vec<ProviderCatalogModel> {
    let items = catalog_items(payload);
    let mut seen = HashSet::new();

    items
        .into_iter()
        .filter_map(|(item, object_key)| {
            let raw_id = item.as_str().or(object_key).or_else(|| {
                item.get("id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("name").and_then(Value::as_str))
                    .or_else(|| item.get("slug").and_then(Value::as_str))
                    .or_else(|| item.get("model").and_then(Value::as_str))
                    .or_else(|| item.get("model_id").and_then(Value::as_str))
                    .or_else(|| item.get("modelId").and_then(Value::as_str))
            })?;
            let id = normalize_model_id(raw_id, protocol);
            if id.is_empty() || !seen.insert(id.clone()) {
                return None;
            }
            let display_name = item
                .get("display_name")
                .and_then(Value::as_str)
                .or_else(|| item.get("displayName").and_then(Value::as_str))
                .unwrap_or(&id)
                .to_string();
            let exact_context_window =
                parse_token_limit(item, &["contextWindow", "context_window"]);
            let max_context_window =
                parse_token_limit(item, &["maxContextWindow", "max_context_window"]);
            let context_length = parse_token_limit(
                item,
                &[
                    "contextLength",
                    "context_length",
                    "maxContextLength",
                    "max_context_length",
                ],
            );
            // 某些目录只返回 max_context_window；context_window 对前端保留一个
            // 可直接展示的有效值，同时单独保留原生硬上限，避免丢失层级信息。
            let context_window = exact_context_window.or(max_context_window);
            let auto_compact_token_limit =
                parse_token_limit(item, &["autoCompactTokenLimit", "auto_compact_token_limit"]);
            let max_tokens = parse_token_limit(item, &["maxTokens", "max_tokens"]);
            let token_budget = parse_token_limit(item, &["tokenBudget", "token_budget"]);
            let explicit_input_token_limit = parse_token_limit(
                item,
                &[
                    "inputTokenLimit",
                    "input_token_limit",
                    "maxInputTokens",
                    "max_input_tokens",
                    "maxPromptTokens",
                    "max_prompt_tokens",
                ],
            );
            let input_token_limit = explicit_input_token_limit.or_else(|| {
                if is_cpa_catalog {
                    context_window.or(context_length)
                } else if matches!(protocol, ProviderProtocol::GeminiGenerateContent) {
                    max_tokens
                } else {
                    None
                }
            });
            let fallback_output_token_limit =
                if matches!(protocol, ProviderProtocol::AnthropicMessages) || is_cpa_catalog {
                    max_tokens
                } else {
                    None
                };
            let output_token_limit = parse_token_limit(
                item,
                &[
                    "outputTokenLimit",
                    "output_token_limit",
                    "maxOutputTokens",
                    "max_output_tokens",
                    "maxCompletionTokens",
                    "max_completion_tokens",
                ],
            )
            .or(fallback_output_token_limit);
            let media = extract_modality_metadata(item);
            Some(ProviderCatalogModel {
                id,
                display_name,
                context_window,
                max_context_window,
                context_length,
                auto_compact_token_limit,
                input_token_limit,
                output_token_limit,
                max_tokens,
                token_budget,
                capabilities: extract_capabilities(item),
                roles: None,
                input_modalities: media.input_modalities,
                output_modalities: media.output_modalities,
                input_mime_types: media.input_mime_types,
                thinking: item.get("thinking").cloned(),
                reasoning: parse_reasoning_metadata(item, protocol),
                upstream_compression: extract_upstream_compression(item),
                default_compression_policy: extract_default_compression_policy(item),
                is_recommended: None,
                agent_sort_order: None,
                is_deprecated: None,
                replacement_model_id: None,
            })
        })
        .collect()
}

/// 兼容 OpenAI/Gemini 的数组目录，也兼容 CPA 常见的模型 ID -> 元数据对象目录。
fn catalog_items(payload: &Value) -> Vec<(&Value, Option<&str>)> {
    if let Some(items) = payload.as_array() {
        return items.iter().map(|item| (item, None)).collect();
    }
    for container in ["data", "models"] {
        let Some(value) = payload.get(container) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            return items.iter().map(|item| (item, None)).collect();
        }
        if let Some(items) = value.as_object() {
            return items
                .iter()
                .map(|(key, item)| (item, Some(key.as_str())))
                .collect();
        }
    }
    payload
        .as_object()
        .filter(|object| object.values().any(Value::is_object))
        .map(|object| {
            object
                .iter()
                .map(|(key, item)| (item, Some(key.as_str())))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(serde::Deserialize)]
struct CheckpointerPayload {
    enabled: Option<bool>,
    token_threshold: Option<Value>,
    max_token_limit: Option<Value>,
    max_output_tokens: Option<Value>,
    checkpoint_model: Option<String>,
    use_last_planner_model: Option<bool>,
    strategy: Option<String>,
    max_overhead_ratio: Option<Value>,
    moving_window_size: Option<Value>,
    is_sync: Option<bool>,
    max_user_requests: Option<Value>,
    include_last_user_message: Option<bool>,
    include_conversation_log: Option<bool>,
    include_running_task_snapshots: Option<bool>,
    include_subagent_snapshots: Option<bool>,
    include_artifact_snapshots: Option<bool>,
    retry_config: Option<CheckpointerRetryPayload>,
}

#[derive(serde::Deserialize)]
struct CheckpointerRetryPayload {
    max_retries: Option<Value>,
    initial_sleep_duration_ms: Option<Value>,
    exponential_multiplier: Option<Value>,
    include_error_feedback: Option<bool>,
}

fn checkpointer_payload(item: &Value) -> Option<CheckpointerPayload> {
    let string_value = item
        .get("modelExperiments")?
        .get("experiments")?
        .get("CASCADE_USE_EXPERIMENT_CHECKPOINTER")?
        .get("stringValue")?
        .as_str()?;
    serde_json::from_str(string_value).ok()
}

pub(super) fn extract_upstream_compression(item: &Value) -> Option<UpstreamCompressionPolicy> {
    let payload = checkpointer_payload(item)?;
    let enabled = payload.enabled?;
    let token_threshold = parse_positive_u32(payload.token_threshold.as_ref()?)?;
    let max_token_limit = parse_positive_u32(payload.max_token_limit.as_ref()?)?;
    let max_output_tokens = match payload.max_output_tokens.as_ref() {
        Some(value) => Some(parse_positive_u32(value)?),
        None => None,
    };

    Some(UpstreamCompressionPolicy {
        enabled,
        token_threshold,
        max_token_limit,
        max_output_tokens,
        checkpoint_model: payload.checkpoint_model,
        use_last_planner_model: payload.use_last_planner_model,
    })
}

fn parse_u32(value: &Value) -> Option<u32> {
    let value = value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|value| value.trim().parse::<u64>().ok())
    })?;
    (value <= u64::from(u32::MAX)).then_some(value as u32)
}

fn parse_number_string(value: &Value) -> Option<String> {
    let string = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    let parsed = string.parse::<f64>().ok()?;
    (parsed.is_finite() && parsed >= 0.0).then_some(string)
}

pub(super) fn extract_default_compression_policy(item: &Value) -> Option<ModelCompressionPolicy> {
    let payload = checkpointer_payload(item)?;
    let retry = payload.retry_config?;
    let policy = ModelCompressionPolicy {
        enabled: payload.enabled?,
        checkpoint_model: payload.checkpoint_model?,
        strategy: payload.strategy?,
        max_overhead_ratio: parse_number_string(payload.max_overhead_ratio.as_ref()?)?,
        moving_window_size: parse_number_string(payload.moving_window_size.as_ref()?)?,
        use_last_planner_model: payload.use_last_planner_model?,
        is_sync: payload.is_sync?,
        max_user_requests: parse_u32(payload.max_user_requests.as_ref()?)?,
        include_last_user_message: payload.include_last_user_message?,
        include_conversation_log: payload.include_conversation_log?,
        include_running_task_snapshots: payload.include_running_task_snapshots?,
        include_subagent_snapshots: payload.include_subagent_snapshots?,
        include_artifact_snapshots: payload.include_artifact_snapshots?,
        retry_config: CustomModelCheckpointRetryConfig {
            max_retries: parse_u32(retry.max_retries.as_ref()?)?,
            initial_sleep_duration_ms: parse_u32(retry.initial_sleep_duration_ms.as_ref()?)?,
            exponential_multiplier: parse_u32(retry.exponential_multiplier.as_ref()?)?,
            include_error_feedback: retry.include_error_feedback?,
        },
        token_threshold: parse_positive_u32(payload.token_threshold.as_ref()?)?,
        max_token_limit: parse_positive_u32(payload.max_token_limit.as_ref()?)?,
        max_output_tokens: parse_positive_u32(payload.max_output_tokens.as_ref()?)?,
    };
    policy.validate("catalog checkpointer").ok()?;
    Some(policy)
}

pub(super) fn parse_official_catalog_models(payload: &Value) -> Vec<ProviderCatalogModel> {
    let response = official_response(payload);
    let Some(models) = response.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };
    let deprecated_model_ids = parse_deprecated_model_ids(response);
    let agent_model_order = parse_agent_model_order(response);
    let (official_roles, has_role_metadata) = parse_official_model_roles(response);
    let has_agent_model_metadata = !agent_model_order.is_empty();

    let mut result = models
        .iter()
        .map(|(model_id, item)| {
            let max_tokens = item.get("maxTokens").and_then(parse_positive_u32);
            let context_window = item.get("contextWindow").and_then(parse_positive_u32);
            let input_token_limit = item
                .get("inputTokenLimit")
                .and_then(parse_positive_u32)
                .or(max_tokens);
            let output_token_limit = ["maxOutputTokens", "outputTokenLimit"]
                .iter()
                .filter_map(|field| item.get(*field).and_then(parse_positive_u32))
                .min();
            let capabilities_object = item.get("capabilities").and_then(Value::as_object);
            let media_field = |keys: &[&str]| {
                keys.iter().find_map(|key| item.get(*key)).or_else(|| {
                    capabilities_object
                        .and_then(|object| keys.iter().find_map(|key| object.get(*key)))
                })
            };
            let supports_image_input = media_field(&[
                "supportsImages",
                "supports_images",
                "supportsVision",
                "supports_vision",
                "supportsImageInput",
                "supports_image_input",
            ])
            .and_then(Value::as_bool);
            let has_explicit_input_modalities =
                media_field(&["inputModalities", "input_modalities"]).is_some();
            let supports_tools = item
                .get("supportsTools")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let supports_reasoning = item.get("supportsThinking").and_then(Value::as_bool);
            let media = extract_modality_metadata(item);
            let mut input_modalities = media.input_modalities;
            let input_modalities =
                input_modalities.get_or_insert_with(|| BTreeSet::from([ModelModality::Text]));
            input_modalities.insert(ModelModality::Text);
            match supports_image_input {
                Some(true) => {
                    input_modalities.insert(ModelModality::Image);
                }
                Some(false) => {
                    input_modalities.remove(&ModelModality::Image);
                }
                None if !has_explicit_input_modalities => {
                    input_modalities.insert(ModelModality::Image);
                }
                None => {}
            }
            let roles = official_roles.get(model_id).cloned().unwrap_or_default();
            let output_modalities = media
                .output_modalities
                .unwrap_or_else(|| official_output_modalities(&roles));
            let input_mime_types = complete_input_mime_types(
                media
                    .input_mime_types
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                input_modalities,
            );

            let mut capabilities = serde_json::json!({
                "inputModalities": input_modalities,
                "outputModalities": output_modalities,
                "tools": supports_tools,
                "raw_config": item,
            });
            if let Some(supports_reasoning) = supports_reasoning {
                capabilities["reasoning"] = Value::Bool(supports_reasoning);
            }

            ProviderCatalogModel {
                id: model_id.clone(),
                display_name: item
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(model_id)
                    .to_string(),
                context_window,
                input_token_limit,
                output_token_limit,
                max_tokens,
                capabilities: Some(capabilities),
                roles: has_role_metadata.then_some(roles),
                input_modalities: Some(input_modalities.clone()),
                output_modalities: Some(output_modalities),
                input_mime_types: (!input_mime_types.is_empty()).then_some(input_mime_types),
                reasoning: parse_reasoning_metadata(item, &ProviderProtocol::GeminiGenerateContent),
                upstream_compression: extract_upstream_compression(item),
                default_compression_policy: extract_default_compression_policy(item),
                is_recommended: item.get("recommended").and_then(Value::as_bool),
                agent_sort_order: agent_model_order.get(model_id).copied(),
                is_deprecated: (!deprecated_model_ids.is_empty())
                    .then(|| deprecated_model_ids.contains_key(model_id)),
                replacement_model_id: deprecated_model_ids.get(model_id).cloned(),
                ..ProviderCatalogModel::default()
            }
        })
        .collect::<Vec<_>>();
    if has_agent_model_metadata {
        result.sort_by(|left, right| {
            left.agent_sort_order
                .unwrap_or(u32::MAX)
                .cmp(&right.agent_sort_order.unwrap_or(u32::MAX))
                .then_with(|| left.id.cmp(&right.id))
        });
    } else {
        result.sort_by(|left, right| left.id.cmp(&right.id));
    }
    result
}

fn official_response(payload: &Value) -> &Value {
    payload
        .get("response")
        .filter(|response| response.get("models").and_then(Value::as_object).is_some())
        .unwrap_or(payload)
}

fn parse_deprecated_model_ids(response: &Value) -> BTreeMap<String, String> {
    response
        .get("deprecatedModelIds")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(old_model_id, value)| {
                    let new_model_id = value.get("newModelId").and_then(Value::as_str)?;
                    (!old_model_id.is_empty() && !new_model_id.is_empty())
                        .then(|| (old_model_id.clone(), new_model_id.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_agent_model_order(response: &Value) -> BTreeMap<String, u32> {
    let mut order = BTreeMap::new();
    let Some(sorts) = response.get("agentModelSorts").and_then(Value::as_array) else {
        return order;
    };

    for sort in sorts {
        let Some(groups) = sort.get("groups").and_then(Value::as_array) else {
            continue;
        };
        for group in groups {
            let Some(model_ids) = group.get("modelIds").and_then(Value::as_array) else {
                continue;
            };
            for model_id in model_ids.iter().filter_map(Value::as_str) {
                if !order.contains_key(model_id) {
                    let position = order.len() as u32;
                    order.insert(model_id.to_string(), position);
                }
            }
        }
    }
    order
}

fn parse_official_model_roles(response: &Value) -> (BTreeMap<String, BTreeSet<ModelRole>>, bool) {
    let mut roles = BTreeMap::<String, BTreeSet<ModelRole>>::new();
    let mut has_metadata = false;

    for model_id in parse_agent_model_order(response).into_keys() {
        has_metadata = true;
        roles.entry(model_id).or_default().insert(ModelRole::Agent);
    }
    if let Some(model_id) = response
        .get("defaultAgentModelId")
        .and_then(Value::as_str)
        .filter(|model_id| !model_id.trim().is_empty())
    {
        has_metadata = true;
        roles
            .entry(model_id.to_string())
            .or_default()
            .insert(ModelRole::Agent);
    }

    for (field, role) in [
        ("commandModelIds", ModelRole::Command),
        ("tabModelIds", ModelRole::Tab),
        ("imageGenerationModelIds", ModelRole::ImageGeneration),
        ("mqueryModelIds", ModelRole::Mquery),
        ("webSearchModelIds", ModelRole::WebSearch),
        ("commitMessageModelIds", ModelRole::CommitMessage),
        ("audioTranscriptionModelIds", ModelRole::AudioTranscription),
    ] {
        let Some(model_ids) = response.get(field).and_then(Value::as_array) else {
            continue;
        };
        for model_id in model_ids
            .iter()
            .filter_map(Value::as_str)
            .filter(|model_id| !model_id.trim().is_empty())
        {
            has_metadata = true;
            roles.entry(model_id.to_string()).or_default().insert(role);
        }
    }

    (roles, has_metadata)
}

fn official_output_modalities(roles: &BTreeSet<ModelRole>) -> BTreeSet<ModelModality> {
    let mut modalities = BTreeSet::new();
    if roles.contains(&ModelRole::ImageGeneration) {
        modalities.insert(ModelModality::Image);
    }
    if roles.is_empty() || roles.iter().any(|role| *role != ModelRole::ImageGeneration) {
        modalities.insert(ModelModality::Text);
    }
    modalities
}

fn extract_capabilities(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    let raw_capabilities = object.get("capabilities");
    let mut capabilities = match raw_capabilities {
        Some(Value::Object(capabilities)) => capabilities.clone(),
        Some(capabilities) => return Some(capabilities.clone()),
        None => Map::new(),
    };

    for key in [
        "inputModalities",
        "input_modalities",
        "outputModalities",
        "output_modalities",
        "supportedGenerationMethods",
        "supported_generation_methods",
        "supportedParameters",
        "supported_parameters",
        "experimentalSupportedTools",
        "experimental_supported_tools",
        "supportsImageInput",
        "supports_image_input",
        "supportsImages",
        "supports_images",
        "supportsVideo",
        "supports_video",
        "supportsAudio",
        "supports_audio",
        "supportedMimeTypes",
        "supported_mime_types",
        "supportsParallelToolCalls",
        "supports_parallel_tool_calls",
        "supportsTools",
        "supports_tools",
        "vision",
        "tools",
    ] {
        if let Some(value) = object.get(key) {
            capabilities
                .entry(key.to_string())
                .or_insert_with(|| value.clone());
        }
    }

    if !capabilities.contains_key("vision") {
        let vision = capability_bool(
            &capabilities,
            &[
                "supportsImageInput",
                "supports_image_input",
                "supportsImages",
                "supports_images",
            ],
        )
        .or_else(|| {
            capability_array_contains(
                &capabilities,
                &["inputModalities", "input_modalities"],
                &["image"],
            )
        });
        if let Some(vision) = vision {
            capabilities.insert("vision".to_string(), Value::Bool(vision));
        }
    }

    if !capabilities.contains_key("tools") {
        let tools = capability_bool(&capabilities, &["supportsTools", "supports_tools"])
            .or_else(|| {
                capability_bool(
                    &capabilities,
                    &["supportsParallelToolCalls", "supports_parallel_tool_calls"],
                )
                .filter(|supported| *supported)
            })
            .or_else(|| {
                capability_array_non_empty(
                    &capabilities,
                    &["experimentalSupportedTools", "experimental_supported_tools"],
                )
                .filter(|supported| *supported)
            })
            .or_else(|| {
                capability_array_contains(
                    &capabilities,
                    &["supportedGenerationMethods", "supported_generation_methods"],
                    &["tool", "function"],
                )
                .filter(|supported| *supported)
            });
        if let Some(tools) = tools {
            capabilities.insert("tools".to_string(), Value::Bool(tools));
        }
    }

    (!capabilities.is_empty()).then_some(Value::Object(capabilities))
}

fn extract_modality_metadata(item: &Value) -> CatalogModalityMetadata {
    let capabilities = item.get("capabilities").and_then(Value::as_object);
    let field = |keys: &[&str]| {
        keys.iter().find_map(|key| item.get(*key)).or_else(|| {
            capabilities.and_then(|object| keys.iter().find_map(|key| object.get(*key)))
        })
    };
    let mime_types = field(&["supportedMimeTypes", "supported_mime_types"])
        .map(parse_supported_mime_types)
        .unwrap_or_default();
    let explicit_input_modalities = field(&["inputModalities", "input_modalities"]);
    let mut input_modalities = extract_modalities(
        explicit_input_modalities,
        [
            (
                ModelModality::Image,
                field(&[
                    "supportsImages",
                    "supports_images",
                    "supportsVision",
                    "supports_vision",
                    "supportsImageInput",
                    "supports_image_input",
                ])
                .and_then(Value::as_bool),
            ),
            (
                ModelModality::Audio,
                field(&["supportsAudio", "supports_audio"]).and_then(Value::as_bool),
            ),
            (
                ModelModality::Video,
                field(&["supportsVideo", "supports_video"]).and_then(Value::as_bool),
            ),
        ],
    );
    if explicit_input_modalities.is_none() {
        if let Some(input_modalities) = input_modalities.as_mut() {
            input_modalities.insert(ModelModality::Text);
        }
    }
    let output_modalities =
        extract_modalities(field(&["outputModalities", "output_modalities"]), []);
    CatalogModalityMetadata {
        input_mime_types: (!mime_types.is_empty()).then(|| mime_types.into_iter().collect()),
        input_modalities,
        output_modalities,
    }
}

fn extract_modalities<const N: usize>(
    explicit: Option<&Value>,
    flags: [(ModelModality, Option<bool>); N],
) -> Option<BTreeSet<ModelModality>> {
    let mut modalities = explicit.map(parse_modalities).unwrap_or_default();
    let has_metadata = explicit.is_some() || flags.iter().any(|(_, value)| value.is_some());
    for (modality, enabled) in flags {
        match enabled {
            Some(true) => {
                modalities.insert(modality);
            }
            Some(false) => {
                modalities.remove(&modality);
            }
            None => {}
        }
    }
    has_metadata.then_some(modalities)
}

fn parse_modalities(value: &Value) -> BTreeSet<ModelModality> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "text" => Some(ModelModality::Text),
            "image" => Some(ModelModality::Image),
            "audio" => Some(ModelModality::Audio),
            "video" => Some(ModelModality::Video),
            "document" | "file" | "pdf" => Some(ModelModality::Document),
            _ => None,
        })
        .collect()
}

fn complete_input_mime_types(
    mut mime_types: BTreeSet<String>,
    input_modalities: &BTreeSet<ModelModality>,
) -> Vec<String> {
    if input_modalities.contains(&ModelModality::Image)
        && !mime_types
            .iter()
            .any(|mime_type| mime_type.starts_with("image/"))
    {
        mime_types.extend(
            ["image/png", "image/jpeg", "image/webp"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if input_modalities.contains(&ModelModality::Audio)
        && !mime_types
            .iter()
            .any(|mime_type| mime_type.starts_with("audio/"))
    {
        mime_types.insert("audio/wav".to_string());
    }
    if input_modalities.contains(&ModelModality::Video)
        && !mime_types
            .iter()
            .any(|mime_type| mime_type.starts_with("video/"))
    {
        mime_types.extend(["video/mp4", "video/webm"].into_iter().map(str::to_string));
    }
    if input_modalities.contains(&ModelModality::Document)
        && !mime_types.contains("application/pdf")
    {
        mime_types.insert("application/pdf".to_string());
    }
    mime_types.into_iter().collect()
}

fn parse_supported_mime_types(value: &Value) -> BTreeSet<String> {
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase)
            .collect(),
        Value::Object(values) => values
            .iter()
            .filter(|(_, enabled)| enabled.as_bool() == Some(true))
            .map(|(mime_type, _)| mime_type.trim().to_ascii_lowercase())
            .filter(|mime_type| !mime_type.is_empty())
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn capability_bool(capabilities: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| capabilities.get(*key).and_then(Value::as_bool))
}

fn capability_array_contains(
    capabilities: &Map<String, Value>,
    keys: &[&str],
    expected_fragments: &[&str],
) -> Option<bool> {
    keys.iter().find_map(|key| {
        capabilities
            .get(*key)
            .and_then(Value::as_array)
            .map(|values| {
                values.iter().filter_map(Value::as_str).any(|value| {
                    let normalized = value.to_ascii_lowercase();
                    expected_fragments
                        .iter()
                        .any(|fragment| normalized.contains(fragment))
                })
            })
    })
}

fn capability_array_non_empty(capabilities: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        capabilities
            .get(*key)
            .and_then(Value::as_array)
            .map(|values| !values.is_empty())
    })
}

fn parse_token_limit(item: &Value, keys: &[&str]) -> Option<u32> {
    fn find(value: &Value, keys: &[&str]) -> Option<u32> {
        let object = value.as_object()?;
        if let Some(limit) = keys
            .iter()
            .filter_map(|key| object.get(*key))
            .find_map(parse_positive_u32)
        {
            return Some(limit);
        }

        for container in [
            "limits",
            "metadata",
            "capabilities",
            "top_provider",
            "topProvider",
        ] {
            if let Some(nested) = object.get(container) {
                if let Some(limit) = find(nested, keys) {
                    return Some(limit);
                }
            }
        }
        None
    }

    find(item, keys)
}

fn parse_positive_u32(value: &Value) -> Option<u32> {
    let value = value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|value| value.trim().parse::<u64>().ok())
    })?;
    (value > 0 && value <= u64::from(u32::MAX)).then_some(value as u32)
}

fn normalize_model_id(value: &str, protocol: &ProviderProtocol) -> String {
    let value = value.trim();
    if matches!(protocol, ProviderProtocol::GeminiGenerateContent) {
        value
            .strip_prefix(MODEL_NAMESPACE_PREFIX)
            .unwrap_or(value)
            .to_string()
    } else {
        value.to_string()
    }
}
