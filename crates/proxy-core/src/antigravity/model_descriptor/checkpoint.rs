use super::AntigravityModelDescriptor;
use crate::domain::ModelCompressionPolicy;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CHECKPOINTER_EXPERIMENT: &str = "CASCADE_USE_EXPERIMENT_CHECKPOINTER";

impl AntigravityModelDescriptor {
    /// Applies configured policies only to matching official catalog entries.
    /// Entries without a model-level policy remain byte-for-byte unchanged.
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        policies: &BTreeMap<String, ModelCompressionPolicy>,
    ) {
        if let Some(models) = models_json.get_mut("models") {
            apply_official_policies(models, policies);
        } else {
            apply_official_policies(models_json, policies);
        }
    }
}

fn apply_official_policies(
    models: &mut Value,
    policies: &BTreeMap<String, ModelCompressionPolicy>,
) {
    match models {
        Value::Object(entries) => {
            for (model_id, entry) in entries {
                let Some(policy) = policies.get(model_id) else {
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
                let Some(policy) = policies.get(model_id) else {
                    continue;
                };
                apply_policy_with_entry_limits(entry, policy);
            }
        }
        _ => {}
    }
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
