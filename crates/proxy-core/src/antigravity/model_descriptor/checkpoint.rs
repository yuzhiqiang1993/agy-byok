use super::AntigravityModelDescriptor;
use crate::domain::ModelCompressionPolicy;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const CHECKPOINTER_EXPERIMENT: &str = "CASCADE_USE_EXPERIMENT_CHECKPOINTER";

impl AntigravityModelDescriptor {
    /// Applies configured policies only to matching official catalog entries.
    /// Entries without a model-level policy remain byte-for-byte unchanged.
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        policies: &BTreeMap<String, ModelCompressionPolicy>,
    ) {
        let aliases = official_model_aliases(models_json);
        if models_json.get("models").is_some() {
            if let Some(models) = models_json.get_mut("models") {
                apply_official_policies(models, policies, &aliases);
            }
        } else if let Some(models) = models_json
            .get_mut("response")
            .and_then(|response| response.get_mut("models"))
        {
            apply_official_policies(models, policies, &aliases);
        } else {
            apply_official_policies(models_json, policies, &aliases);
        }
    }
}

fn apply_official_policies(
    models: &mut Value,
    policies: &BTreeMap<String, ModelCompressionPolicy>,
    aliases: &BTreeMap<String, String>,
) {
    match models {
        Value::Object(entries) => {
            for (model_id, entry) in entries {
                let Some(policy) = effective_policy(model_id, policies, aliases) else {
                    continue;
                };
                apply_policy_with_entry_limits(entry, policy);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                let Some(model_id) = entry.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(policy) = effective_policy(model_id, policies, aliases) else {
                    continue;
                };
                apply_policy_with_entry_limits(entry, policy);
            }
        }
        _ => {}
    }
}

fn effective_policy<'a>(
    model_id: &str,
    policies: &'a BTreeMap<String, ModelCompressionPolicy>,
    aliases: &BTreeMap<String, String>,
) -> Option<&'a ModelCompressionPolicy> {
    // 同一逻辑模型的旧、新条目共享压缩策略；规范 ID 配置优先。
    let canonical_id = canonical_model_id(model_id, aliases);
    policies
        .get(canonical_id)
        .or_else(|| policies.get(model_id))
        .or_else(|| {
            aliases.iter().find_map(|(deprecated_id, replacement_id)| {
                if canonical_model_id(deprecated_id, aliases) != canonical_id {
                    return None;
                }
                policies
                    .get(deprecated_id)
                    .or_else(|| policies.get(replacement_id))
            })
        })
}

fn canonical_model_id<'a>(model_id: &'a str, aliases: &'a BTreeMap<String, String>) -> &'a str {
    let mut current = model_id;
    let mut visited = BTreeSet::new();
    while let Some(next) = aliases.get(current) {
        if !visited.insert(current) {
            break;
        }
        current = next;
    }
    current
}

fn official_model_aliases(models_json: &Value) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let containers = models_json
        .get("response")
        .filter(|response| response.is_object())
        .map_or_else(|| vec![models_json], |response| vec![models_json, response]);
    for container in containers {
        let Some(entries) = container
            .get("deprecatedModelIds")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (deprecated_id, value) in entries {
            let Some(replacement_id) = value.get("newModelId").and_then(Value::as_str) else {
                continue;
            };
            if !deprecated_id.is_empty() && !replacement_id.is_empty() {
                aliases.insert(deprecated_id.clone(), replacement_id.to_string());
            }
        }
    }
    aliases
}

fn apply_policy_with_entry_limits(entry: &mut Value, policy: &ModelCompressionPolicy) {
    let capacity =
        minimum_positive_field(entry, &["maxTokens", "inputTokenLimit", "contextWindow"])
            .unwrap_or(policy.max_token_limit);
    let output_token_limit =
        minimum_positive_field(entry, &["maxOutputTokens", "outputTokenLimit"])
            .unwrap_or(policy.max_output_tokens);
    apply_model_compression_policy(entry, policy, capacity, output_token_limit);
}

pub(super) fn apply_model_compression_policy(
    entry: &mut Value,
    policy: &ModelCompressionPolicy,
    capacity: u32,
    output_token_limit: u32,
) {
    let Some((token_threshold, max_token_limit, max_output_tokens)) =
        clamp_policy_limits(policy, capacity, output_token_limit)
    else {
        return;
    };

    let mut payload =
        serde_json::to_value(policy).expect("model compression policy serialization cannot fail");
    let payload = payload
        .as_object_mut()
        .expect("model compression policy must serialize as an object");
    payload.insert(
        "token_threshold".to_string(),
        Value::String(token_threshold.to_string()),
    );
    payload.insert(
        "max_token_limit".to_string(),
        Value::String(max_token_limit.to_string()),
    );
    payload.insert(
        "max_output_tokens".to_string(),
        Value::String(max_output_tokens.to_string()),
    );

    let Some(entry) = entry.as_object_mut() else {
        return;
    };
    let experiment = entry
        .entry("modelExperiments")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .and_then(|model_experiments| {
            model_experiments
                .entry("experiments")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .and_then(|experiments| {
            experiments
                .entry(CHECKPOINTER_EXPERIMENT)
                .or_insert_with(|| json!({}))
                .as_object_mut()
        });
    let Some(experiment) = experiment else {
        return;
    };
    experiment.insert(
        "stringValue".to_string(),
        Value::String(
            serde_json::to_string(payload)
                .expect("model compression policy payload serialization cannot fail"),
        ),
    );
}

fn clamp_policy_limits(
    policy: &ModelCompressionPolicy,
    capacity: u32,
    output_token_limit: u32,
) -> Option<(u32, u32, u32)> {
    if capacity < 2 || output_token_limit == 0 {
        return None;
    }

    let max_token_limit = policy.max_token_limit.min(capacity);
    if max_token_limit < 2 {
        return None;
    }
    let max_output_tokens = policy
        .max_output_tokens
        .min(output_token_limit)
        .min(max_token_limit.saturating_sub(1));
    if max_output_tokens == 0 {
        return None;
    }
    let token_threshold = policy
        .token_threshold
        .min(max_token_limit.saturating_sub(max_output_tokens));
    (token_threshold > 0).then_some((token_threshold, max_token_limit, max_output_tokens))
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
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<u32>().ok())
        })
        .filter(|value| *value > 0)
}
