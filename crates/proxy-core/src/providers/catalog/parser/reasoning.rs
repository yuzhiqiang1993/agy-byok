use super::super::ProviderCatalogReasoning;
use super::parse_positive_u32;
use crate::domain::{ProviderProtocol, ReasoningLevel, ReasoningMapping};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn parse_reasoning_metadata(
    item: &Value,
    protocol: &ProviderProtocol,
) -> Option<ProviderCatalogReasoning> {
    let object = item.as_object()?;
    let mut supported = None;
    let mut levels = Vec::new();
    let mut mappings = BTreeMap::new();
    for key in [
        "reasoning",
        "thinking",
        "reasoning_capability",
        "reasoningCapability",
        "supports_reasoning",
        "supportsReasoning",
        "reasoning_levels",
        "reasoningLevels",
        "supported_reasoning_levels",
        "supportedReasoningLevels",
        "thinking_levels",
        "thinkingLevels",
        "supported_thinking_levels",
        "supportedThinkingLevels",
        "reasoning_effort",
        "reasoningEffort",
        "effort",
        "supported_efforts",
        "supportedEfforts",
    ] {
        if let Some(value) = object.get(key) {
            collect_reasoning_metadata(value, protocol, &mut supported, &mut levels, &mut mappings);
        }
    }

    for key in ["type", "model_type", "modelType"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let value = value.to_ascii_lowercase();
            if value.contains("reasoning") || value.contains("thinking") {
                supported = Some(true);
            }
        }
    }

    if let Some(capabilities) = object.get("capabilities") {
        if let Some(capabilities) = capabilities.as_object() {
            for key in [
                "reasoning",
                "thinking",
                "reasoning_levels",
                "reasoningLevels",
                "effort",
                "supported_efforts",
                "supportedEfforts",
            ] {
                if let Some(value) = capabilities.get(key) {
                    collect_reasoning_metadata(
                        value,
                        protocol,
                        &mut supported,
                        &mut levels,
                        &mut mappings,
                    );
                }
            }
        } else if let Some(capabilities) = capabilities.as_array() {
            if capabilities.iter().filter_map(Value::as_str).any(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("reasoning") || value.contains("thinking")
            }) {
                supported = Some(true);
            }
        }
    }

    if let Some(parameters) = object.get("supported_parameters").and_then(Value::as_array) {
        if parameters
            .iter()
            .filter_map(Value::as_str)
            .any(|parameter| {
                let parameter = parameter.to_ascii_lowercase();
                parameter.contains("reasoning") || parameter.contains("thinking")
            })
        {
            supported = Some(true);
        }
    }

    levels.sort();
    levels.dedup();
    if !levels.is_empty() || !mappings.is_empty() {
        supported = Some(true);
    }
    if supported.is_none() && levels.is_empty() && mappings.is_empty() {
        return None;
    }
    Some(ProviderCatalogReasoning {
        supported,
        levels,
        mappings,
    })
}

fn collect_reasoning_metadata(
    value: &Value,
    protocol: &ProviderProtocol,
    supported: &mut Option<bool>,
    levels: &mut Vec<ReasoningLevel>,
    mappings: &mut BTreeMap<ReasoningLevel, ReasoningMapping>,
) {
    match value {
        Value::Bool(value) => *supported = Some(*value),
        Value::String(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "enabled" | "supported" | "true" | "on" => *supported = Some(true),
                "disabled" | "unsupported" | "none" | "false" | "off" => *supported = Some(false),
                _ => add_reasoning_level(value, protocol, None, levels, mappings),
            }
        }
        Value::Number(value) if value.as_u64().is_some_and(|value| value > 0) => {
            *supported = Some(true);
        }
        Value::Array(values) => {
            for value in values {
                collect_reasoning_metadata(value, protocol, supported, levels, mappings);
            }
        }
        Value::Object(object) => {
            for key in [
                "levels",
                "supported_levels",
                "supportedLevels",
                "reasoning_levels",
                "reasoningLevels",
                "effort",
                "supported_efforts",
                "supportedEfforts",
                "modes",
                "supported_modes",
                "supportedModes",
                "types",
            ] {
                if let Some(value) = object.get(key) {
                    collect_reasoning_metadata(value, protocol, supported, levels, mappings);
                }
            }
            for (key, value) in object {
                let Some(level) = normalize_reasoning_level(key) else {
                    continue;
                };
                let budget_tokens = parse_reasoning_budget(value);
                if value.as_bool() == Some(false) {
                    continue;
                }
                add_reasoning_level(key, protocol, budget_tokens, levels, mappings);
                if level != ReasoningLevel::Off {
                    *supported = Some(true);
                }
            }
            for key in [
                "supported",
                "enabled",
                "supports_reasoning",
                "supportsReasoning",
            ] {
                if let Some(value) = object.get(key).and_then(Value::as_bool) {
                    *supported = Some(value);
                }
            }
            if let Some(value) = object.get("type").and_then(Value::as_str) {
                match value.to_ascii_lowercase().as_str() {
                    "enabled" | "adaptive" => *supported = Some(true),
                    "disabled" | "none" => *supported = Some(false),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn add_reasoning_level(
    value: &str,
    protocol: &ProviderProtocol,
    budget_tokens: Option<u32>,
    levels: &mut Vec<ReasoningLevel>,
    mappings: &mut BTreeMap<ReasoningLevel, ReasoningMapping>,
) {
    let Some(level) = normalize_reasoning_level(value) else {
        return;
    };
    if !levels.contains(&level) {
        levels.push(level);
    }
    let Some(mapping) = reasoning_mapping(protocol, level, value, budget_tokens) else {
        return;
    };
    if budget_tokens.is_some() {
        mappings.insert(level, mapping);
    } else {
        mappings.entry(level).or_insert(mapping);
    }
}

fn normalize_reasoning_level(value: &str) -> Option<ReasoningLevel> {
    let normalized = value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-' && *character != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "off" | "none" => Some(ReasoningLevel::Off),
        "low" | "minimal" => Some(ReasoningLevel::Low),
        "medium" | "med" | "balanced" => Some(ReasoningLevel::Medium),
        "high" => Some(ReasoningLevel::High),
        "xhigh" | "extrahigh" => Some(ReasoningLevel::XHigh),
        "max" | "maximum" => Some(ReasoningLevel::Max),
        "auto" | "adaptive" => Some(ReasoningLevel::Auto),
        _ => None,
    }
}

fn parse_reasoning_budget(value: &Value) -> Option<u32> {
    if let Some(budget) = parse_positive_u32(value) {
        return Some(budget);
    }
    let object = value.as_object()?;
    [
        "budget_tokens",
        "budgetTokens",
        "thinking_budget",
        "thinkingBudget",
        "max_thinking_tokens",
        "maxThinkingTokens",
    ]
    .iter()
    .filter_map(|key| object.get(*key))
    .find_map(parse_positive_u32)
}

fn level_name(level: ReasoningLevel) -> &'static str {
    match level {
        ReasoningLevel::Off => "off",
        ReasoningLevel::Low => "low",
        ReasoningLevel::Medium => "medium",
        ReasoningLevel::High => "high",
        ReasoningLevel::XHigh => "xhigh",
        ReasoningLevel::Max => "max",
        ReasoningLevel::Auto => "auto",
    }
}

fn reasoning_mapping(
    protocol: &ProviderProtocol,
    level: ReasoningLevel,
    native_value: &str,
    budget_tokens: Option<u32>,
) -> Option<ReasoningMapping> {
    if let Some(budget_tokens) = budget_tokens {
        return Some(ReasoningMapping::BudgetTokens(budget_tokens));
    }
    if level == ReasoningLevel::Off {
        return Some(ReasoningMapping::Disabled);
    }
    if level == ReasoningLevel::Auto && matches!(protocol, ProviderProtocol::AnthropicMessages) {
        return Some(ReasoningMapping::Adaptive);
    }

    let native_value = if native_value.trim().is_empty() {
        level_name(level).to_string()
    } else {
        native_value.trim().to_ascii_lowercase()
    };
    match protocol {
        ProviderProtocol::AnthropicMessages => Some(ReasoningMapping::Effort(native_value)),
        ProviderProtocol::GeminiGenerateContent => {
            Some(ReasoningMapping::NativeLevel(native_value))
        }
        ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
            Some(ReasoningMapping::Effort(native_value))
        }
    }
}
