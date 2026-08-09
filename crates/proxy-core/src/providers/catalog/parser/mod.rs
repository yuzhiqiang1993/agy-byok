mod reasoning;

use super::{ProviderCatalogModel, UpstreamCompressionPolicy};
use crate::domain::ProviderProtocol;
use reasoning::parse_reasoning_metadata;
use serde_json::{Map, Value};
use std::collections::HashSet;

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
                thinking: item.get("thinking").cloned(),
                reasoning: parse_reasoning_metadata(item, protocol),
                upstream_compression: extract_upstream_compression(item),
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
}

pub(super) fn extract_upstream_compression(item: &Value) -> Option<UpstreamCompressionPolicy> {
    let string_value = item
        .get("modelExperiments")?
        .get("experiments")?
        .get("CASCADE_USE_EXPERIMENT_CHECKPOINTER")?
        .get("stringValue")?
        .as_str()?;
    let payload: CheckpointerPayload = serde_json::from_str(string_value).ok()?;
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
    })
}

pub(super) fn parse_official_catalog_models(payload: &Value) -> Vec<ProviderCatalogModel> {
    let Some(models) = payload
        .get("response")
        .and_then(|response| response.get("models"))
        .or_else(|| payload.get("models"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

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
            let supports_vision = item
                .get("supportsVision")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let supports_tools = item
                .get("supportsTools")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let supports_reasoning = item
                .get("supportsThinking")
                .and_then(Value::as_bool)
                .unwrap_or(true);

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
                capabilities: Some(serde_json::json!({
                    "vision": supports_vision,
                    "tools": supports_tools,
                    "reasoning": supports_reasoning,
                    "raw_config": item,
                })),
                upstream_compression: extract_upstream_compression(item),
                ..ProviderCatalogModel::default()
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.id.cmp(&right.id));
    result
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
        "supportedGenerationMethods",
        "supported_generation_methods",
        "supportedParameters",
        "supported_parameters",
        "experimentalSupportedTools",
        "experimental_supported_tools",
        "supportsImageInput",
        "supports_image_input",
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
            &["supportsImageInput", "supports_image_input"],
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
        value.strip_prefix("models/").unwrap_or(value).to_string()
    } else {
        value.to_string()
    }
}
