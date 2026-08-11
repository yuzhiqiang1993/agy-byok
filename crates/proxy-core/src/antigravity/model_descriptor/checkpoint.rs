use super::{catalog_models, catalog_models_mut, AntigravityModelDescriptor};
use crate::domain::ModelCompressionPolicy;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const CHECKPOINTER_EXPERIMENT: &str = "CASCADE_USE_EXPERIMENT_CHECKPOINTER";

impl AntigravityModelDescriptor {
    pub fn remove_disabled_official_models(
        models_json: &mut Value,
        disabled_models: &std::collections::HashSet<String>,
    ) {
        if disabled_models.is_empty() {
            return;
        }
        let aliases = official_model_aliases(models_json);
        let container = super::catalog_container_mut(models_json);
        if let Some(models) = container.get_mut("models") {
            match models {
                Value::Object(map) => {
                    map.retain(|key, _| {
                        let canonical = canonical_model_id(key, &aliases);
                        !disabled_models.contains(key) && !disabled_models.contains(canonical)
                    });
                }
                Value::Array(arr) => {
                    arr.retain(|item| {
                        let id = item
                            .get("id")
                            .or_else(|| item.get("model"))
                            .and_then(Value::as_str);
                        if let Some(id) = id {
                            let clean_id = id.strip_prefix("models/").unwrap_or(id);
                            let canonical = canonical_model_id(clean_id, &aliases);
                            !disabled_models.contains(id)
                                && !disabled_models.contains(clean_id)
                                && !disabled_models.contains(canonical)
                        } else {
                            true
                        }
                    });
                }
                _ => {}
            }

            if let Some(model_sorts) = container
                .get_mut("agentModelSorts")
                .and_then(Value::as_array_mut)
            {
                for sort in model_sorts {
                    if let Some(groups) = sort.get_mut("groups").and_then(Value::as_array_mut) {
                        for group in groups {
                            if let Some(model_ids) =
                                group.get_mut("modelIds").and_then(Value::as_array_mut)
                            {
                                model_ids.retain(|mid| {
                                    mid.as_str().map_or(true, |id| {
                                        let clean_id = id.strip_prefix("models/").unwrap_or(id);
                                        let canonical = canonical_model_id(clean_id, &aliases);
                                        !disabled_models.contains(id)
                                            && !disabled_models.contains(clean_id)
                                            && !disabled_models.contains(canonical)
                                    })
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// Applies configured policies only to matching official catalog entries.
    /// Entries without a model-level policy remain byte-for-byte unchanged.
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        policies: &BTreeMap<String, ModelCompressionPolicy>,
    ) {
        let aliases = official_model_aliases(models_json);
        let checkpoint_output_limits = official_checkpoint_output_limits(models_json);
        apply_official_policies(
            catalog_models_mut(models_json),
            policies,
            &aliases,
            &checkpoint_output_limits,
        );
    }
}

fn apply_official_policies(
    models: &mut Value,
    policies: &BTreeMap<String, ModelCompressionPolicy>,
    aliases: &BTreeMap<String, String>,
    checkpoint_output_limits: &BTreeMap<String, u32>,
) {
    match models {
        Value::Object(entries) => {
            for (model_id, entry) in entries {
                let Some(policy) = effective_policy(model_id, policies, aliases) else {
                    continue;
                };
                if !policy.enabled {
                    continue;
                }
                apply_policy_with_entry_limits(entry, policy, checkpoint_output_limits, aliases);
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
                if !policy.enabled {
                    continue;
                }
                apply_policy_with_entry_limits(entry, policy, checkpoint_output_limits, aliases);
            }
        }
        _ => {}
    }
}

pub(super) fn official_checkpoint_output_limits(models_json: &Value) -> BTreeMap<String, u32> {
    let models = catalog_models(models_json);
    let mut limits = BTreeMap::new();

    let process_entry = |limits: &mut BTreeMap<String, u32>, model_id: &str, entry: &Value| {
        let Some(output_limit) =
            minimum_positive_field(entry, &["maxOutputTokens", "outputTokenLimit"])
        else {
            return;
        };
        limits.insert(model_id.to_string(), output_limit);
        for field in ["id", "model"] {
            if let Some(identifier) = entry.get(field).and_then(Value::as_str) {
                limits.insert(identifier.to_string(), output_limit);
            }
        }
    };

    match models {
        Value::Object(entries) => {
            for (model_id, entry) in entries {
                process_entry(&mut limits, model_id, entry);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                if let Some(model_id) = entry
                    .get("id")
                    .or_else(|| entry.get("model"))
                    .and_then(Value::as_str)
                {
                    process_entry(&mut limits, model_id, entry);
                }
            }
        }
        _ => {}
    }
    limits
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

pub(super) fn canonical_model_id<'a>(
    model_id: &'a str,
    aliases: &'a BTreeMap<String, String>,
) -> &'a str {
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

pub(super) fn official_model_aliases(models_json: &Value) -> BTreeMap<String, String> {
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

fn apply_policy_with_entry_limits(
    entry: &mut Value,
    policy: &ModelCompressionPolicy,
    checkpoint_output_limits: &BTreeMap<String, u32>,
    aliases: &BTreeMap<String, String>,
) {
    let capacity =
        minimum_positive_field(entry, &["maxTokens", "inputTokenLimit", "contextWindow"])
            .unwrap_or(policy.max_token_limit);
    let entry_output_limit =
        minimum_positive_field(entry, &["maxOutputTokens", "outputTokenLimit"])
            .unwrap_or(policy.max_output_tokens);
    let checkpoint_model = existing_checkpoint_payload(entry)
        .and_then(|payload| {
            payload
                .get("checkpoint_model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| policy.checkpoint_model.clone());
    let canonical_checkpoint = canonical_model_id(&checkpoint_model, aliases);
    let output_token_limit = checkpoint_output_limits
        .get(canonical_checkpoint)
        .or_else(|| checkpoint_output_limits.get(&checkpoint_model))
        .copied()
        .map_or(entry_output_limit, |checkpoint_limit| {
            entry_output_limit.min(checkpoint_limit)
        });
    apply_model_compression_policy(entry, policy, capacity, output_token_limit, None);
}

pub(super) fn apply_model_compression_policy(
    entry: &mut Value,
    policy: &ModelCompressionPolicy,
    capacity: u32,
    output_token_limit: u32,
    fallback_template: Option<&ModelCompressionPolicy>,
) {
    let Some(resolved) = policy.resolve_effective(Some(capacity), Some(output_token_limit)) else {
        return;
    };
    let token_threshold = resolved.token_threshold;
    let max_token_limit = resolved.max_token_limit;
    let max_output_tokens = resolved.max_output_tokens;

    let mut payload = match existing_checkpoint_payload(entry) {
        Some(payload) => payload,
        None if fallback_template.is_some() => serde_json::to_value(
            fallback_template.expect("checked fallback compression policy must exist"),
        )
        .expect("fallback compression policy serialization cannot fail")
        .as_object()
        .expect("fallback compression policy must serialize as an object")
        .clone(),
        None => return,
    };
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
            serde_json::to_string(&payload)
                .expect("model compression policy payload serialization cannot fail"),
        ),
    );
}

fn existing_checkpoint_payload(entry: &Value) -> Option<serde_json::Map<String, Value>> {
    entry
        .get("modelExperiments")
        .and_then(|model_experiments| model_experiments.get("experiments"))
        .and_then(|experiments| experiments.get(CHECKPOINTER_EXPERIMENT))
        .and_then(|experiment| experiment.get("stringValue"))
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|payload| payload.as_object().cloned())
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
