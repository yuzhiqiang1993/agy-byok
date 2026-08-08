use super::AntigravityModelDescriptor;
use crate::domain::model::ClaudeCheckpointMetadata;
use crate::domain::OfficialModelSettings;
use serde_json::{json, Value};

impl AntigravityModelDescriptor {
    /// 覆盖官方 Gemini 与 Claude 模型目录中的检查点参数。
    ///
    /// Antigravity IDE 会从 `modelExperiments` 中读取检查点策略；这与实际
    /// 生成请求的 `max_tokens` 不是同一层配置。官方档位不做任何改写，避免
    /// 上游将来的参数变化被本地默认值遮蔽。
    pub fn apply_official_model_overrides(
        models_json: &mut Value,
        settings: &OfficialModelSettings,
    ) {
        if models_json.get("models").is_some() {
            if let Some(models) = models_json.get_mut("models") {
                apply_checkpoint_overrides_to_models(models, settings);
            }
        } else {
            apply_checkpoint_overrides_to_models(models_json, settings);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficialModelFamily {
    Gemini,
    Claude,
}

fn apply_checkpoint_overrides_to_models(models: &mut Value, settings: &OfficialModelSettings) {
    match models {
        Value::Object(entries) => {
            for (key, entry) in entries {
                apply_official_checkpoint_override(Some(key), entry, settings);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                apply_official_checkpoint_override(None, entry, settings);
            }
        }
        _ => {}
    }
}

fn apply_official_checkpoint_override(
    key: Option<&str>,
    entry: &mut Value,
    settings: &OfficialModelSettings,
) {
    let Some(family) = official_model_family(key, entry) else {
        return;
    };
    let limits = match family {
        OfficialModelFamily::Gemini => settings.gemini_checkpoint_limits(),
        OfficialModelFamily::Claude => claude_checkpoint_metadata(entry)
            .and_then(|metadata| settings.claude_checkpoint_limits(metadata)),
    };
    let Some(limits) = limits else {
        return;
    };
    let fallback_checkpoint_model = match family {
        OfficialModelFamily::Gemini => "MODEL_GEMINI",
        OfficialModelFamily::Claude => key.unwrap_or("MODEL_CLAUDE"),
    };
    apply_checkpoint_override(
        entry,
        limits.token_threshold,
        limits.max_token_limit,
        limits.max_output_tokens,
        fallback_checkpoint_model,
    );
}

fn official_model_family(key: Option<&str>, entry: &Value) -> Option<OfficialModelFamily> {
    let mut candidates = Vec::with_capacity(8);
    if let Some(key) = key {
        candidates.push(key);
    }
    for field in [
        "id",
        "model",
        "modelId",
        "requestedModel",
        "planModel",
        "displayName",
        "name",
    ] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            candidates.push(value);
        }
    }
    if candidates
        .iter()
        .any(|value| is_custom_model_identity(value))
    {
        return None;
    }

    let mut is_gemini = false;
    let mut is_claude = false;
    for value in candidates {
        mark_model_family(value, &mut is_gemini, &mut is_claude);
    }
    if let Some(checkpoint_model) = existing_checkpoint(entry)
        .as_ref()
        .and_then(|checkpoint| checkpoint.get("checkpoint_model"))
        .and_then(Value::as_str)
    {
        mark_model_family(checkpoint_model, &mut is_gemini, &mut is_claude);
    }
    for field in ["modelProvider", "apiProvider"] {
        if entry
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| value.to_ascii_lowercase().contains("anthropic"))
        {
            is_claude = true;
        }
    }

    match (is_gemini, is_claude) {
        (true, false) => Some(OfficialModelFamily::Gemini),
        (false, true) => Some(OfficialModelFamily::Claude),
        _ => None,
    }
}

fn mark_model_family(value: &str, is_gemini: &mut bool, is_claude: &mut bool) {
    let normalized = value.to_ascii_lowercase();
    *is_gemini |= normalized.contains("gemini");
    *is_claude |= normalized.contains("claude");
}

fn is_custom_model_identity(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    if normalized.starts_with("custom-") {
        return true;
    }
    let model_id = normalized.strip_prefix("models/").unwrap_or(&normalized);
    model_id
        .strip_prefix("model_placeholder_m")
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (400..600).contains(&value))
}

fn claude_checkpoint_metadata(entry: &Value) -> Option<ClaudeCheckpointMetadata> {
    let capacity =
        minimum_positive_field(entry, &["maxTokens", "inputTokenLimit", "contextWindow"])?;
    let output_token_limit =
        minimum_positive_field(entry, &["maxOutputTokens", "outputTokenLimit"]);

    Some(ClaudeCheckpointMetadata {
        capacity,
        output_token_limit,
    })
}

fn existing_checkpoint(entry: &Value) -> Option<Value> {
    entry
        .get("modelExperiments")?
        .get("experiments")?
        .get("CASCADE_USE_EXPERIMENT_CHECKPOINTER")?
        .get("stringValue")?
        .as_str()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
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
        .or_else(|| value.as_str().and_then(|value| value.parse::<u32>().ok()))
        .filter(|value| *value > 0)
}

fn apply_checkpoint_override(
    entry: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
    fallback_checkpoint_model: &str,
) {
    let checkpoint_model = existing_checkpoint(entry)
        .as_ref()
        .and_then(|checkpoint| checkpoint.get("checkpoint_model"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            ["model", "modelId", "requestedModel", "planModel", "id"]
                .iter()
                .find_map(|field| entry.get(*field).and_then(Value::as_str))
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback_checkpoint_model.to_string());
    apply_checkpoint_override_with_model(
        entry,
        threshold,
        max_token_limit,
        max_output_tokens,
        &checkpoint_model,
    );
}

pub(super) fn apply_checkpoint_override_with_model(
    entry: &mut Value,
    threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
    checkpoint_model: &str,
) {
    let Some(entry_object) = entry.as_object_mut() else {
        return;
    };
    let experiment = entry_object
        .entry("modelExperiments")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .and_then(|experiments| {
            experiments
                .entry("experiments")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .and_then(|experiments| {
            experiments
                .entry("CASCADE_USE_EXPERIMENT_CHECKPOINTER")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        });
    let Some(experiment) = experiment else {
        return;
    };

    let mut checkpoint = experiment
        .get("stringValue")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let Some(checkpoint) = checkpoint.as_object_mut() else {
        return;
    };
    checkpoint.insert(
        "strategy".to_string(),
        Value::String("CHECKPOINT_STRATEGY_UNSPECIFIED".to_string()),
    );
    checkpoint.insert(
        "token_threshold".to_string(),
        Value::String(threshold.to_string()),
    );
    checkpoint.insert(
        "max_token_limit".to_string(),
        Value::String(max_token_limit.to_string()),
    );
    checkpoint.insert(
        "max_output_tokens".to_string(),
        Value::String(max_output_tokens.to_string()),
    );
    checkpoint
        .entry("max_overhead_ratio")
        .or_insert_with(|| json!(0.15));
    checkpoint
        .entry("moving_window_size")
        .or_insert_with(|| json!(1));
    checkpoint.insert(
        "checkpoint_model".to_string(),
        Value::String(checkpoint_model.to_string()),
    );
    if let Ok(serialized) = serde_json::to_string(&Value::Object(checkpoint.clone())) {
        experiment.insert("stringValue".to_string(), Value::String(serialized));
    }
}
