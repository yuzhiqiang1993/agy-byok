const GEMINI_CONTEXT_WINDOW_LIMIT: u32 = 1_048_576;
const DEFAULT_TOKEN_THRESHOLD_PERCENT: u8 = 61;
const DEFAULT_MAX_TOKEN_LIMIT_PERCENT: u8 = 73;
const DEFAULT_MAX_OUTPUT_TOKENS_PERCENT: u8 = 2;
const SUPPORTED_CHECKPOINT_MODELS: [&str; 3] = [
    "MODEL_PLACEHOLDER_M50",
    "MODEL_PLACEHOLDER_M71",
    "MODEL_PLACEHOLDER_M72",
];

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointLimitMode {
    Percentage,
    Absolute,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompressionLimitsPolicy {
    pub enabled: bool,
    pub mode: CheckpointLimitMode,
    pub token_threshold_percent: u8,
    pub max_token_limit_percent: u8,
    pub max_output_tokens_percent: u8,
    pub token_threshold: u32,
    pub max_token_limit: u32,
    pub max_output_tokens: u32,
}

impl CompressionLimitsPolicy {
    pub const fn percentage(enabled: bool) -> Self {
        Self {
            enabled,
            mode: CheckpointLimitMode::Percentage,
            token_threshold_percent: DEFAULT_TOKEN_THRESHOLD_PERCENT,
            max_token_limit_percent: DEFAULT_MAX_TOKEN_LIMIT_PERCENT,
            max_output_tokens_percent: DEFAULT_MAX_OUTPUT_TOKENS_PERCENT,
            token_threshold: 0,
            max_token_limit: 0,
            max_output_tokens: 0,
        }
    }

    pub fn validate(&self, scope: &str) -> Result<(), String> {
        match self.mode {
            CheckpointLimitMode::Percentage => {
                for (name, value) in [
                    ("token_threshold_percent", self.token_threshold_percent),
                    ("max_token_limit_percent", self.max_token_limit_percent),
                    ("max_output_tokens_percent", self.max_output_tokens_percent),
                ] {
                    if !(1..=100).contains(&value) {
                        return Err(format!("{scope} {name} must be between 1 and 100"));
                    }
                }
                if self.token_threshold_percent >= self.max_token_limit_percent {
                    return Err(format!(
                        "{scope} token_threshold_percent must be less than max_token_limit_percent"
                    ));
                }
                if self.max_output_tokens_percent >= self.max_token_limit_percent {
                    return Err(format!(
                        "{scope} max_output_tokens_percent must be less than max_token_limit_percent"
                    ));
                }
                if u16::from(self.token_threshold_percent)
                    + u16::from(self.max_output_tokens_percent)
                    > u16::from(self.max_token_limit_percent)
                {
                    return Err(format!(
                        "{scope} threshold and output reserve percentages exceed the hard limit"
                    ));
                }
            }
            CheckpointLimitMode::Absolute => {
                validate_absolute_limits(
                    scope,
                    self.token_threshold,
                    self.max_token_limit,
                    self.max_output_tokens,
                )?;
            }
        }
        Ok(())
    }
}

impl Default for CompressionLimitsPolicy {
    fn default() -> Self {
        Self::percentage(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustomModelCheckpointRetryConfig {
    pub max_retries: u32,
    pub initial_sleep_duration_ms: u32,
    pub exponential_multiplier: u32,
    pub include_error_feedback: bool,
}

impl Default for CustomModelCheckpointRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 0,
            initial_sleep_duration_ms: 1_000,
            exponential_multiplier: 2,
            include_error_feedback: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CheckpointExecutionPolicy {
    pub enabled: bool,
    pub checkpoint_model: String,
    pub strategy: String,
    pub max_overhead_ratio: String,
    pub moving_window_size: String,
    pub use_last_planner_model: bool,
    pub is_sync: bool,
    pub max_user_requests: u32,
    pub include_last_user_message: bool,
    pub include_conversation_log: bool,
    pub include_running_task_snapshots: bool,
    pub include_subagent_snapshots: bool,
    pub include_artifact_snapshots: bool,
    pub retry_config: CustomModelCheckpointRetryConfig,
}

impl Default for CheckpointExecutionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            checkpoint_model: "MODEL_PLACEHOLDER_M71".to_string(),
            strategy: "CHECKPOINT_STRATEGY_UNSPECIFIED".to_string(),
            max_overhead_ratio: "0.15".to_string(),
            moving_window_size: "1".to_string(),
            use_last_planner_model: false,
            is_sync: false,
            max_user_requests: 10,
            include_last_user_message: false,
            include_conversation_log: true,
            include_running_task_snapshots: true,
            include_subagent_snapshots: true,
            include_artifact_snapshots: true,
            retry_config: CustomModelCheckpointRetryConfig::default(),
        }
    }
}

impl CheckpointExecutionPolicy {
    pub fn validate(&self, scope: &str) -> Result<(), String> {
        if !SUPPORTED_CHECKPOINT_MODELS.contains(&self.checkpoint_model.as_str()) {
            return Err(format!(
                "{scope} checkpoint_model must be one of {}",
                SUPPORTED_CHECKPOINT_MODELS.join(", ")
            ));
        }
        if self.strategy.trim().is_empty() {
            return Err(format!("{scope} strategy cannot be empty"));
        }
        validate_non_negative_number(scope, "max_overhead_ratio", &self.max_overhead_ratio)?;
        validate_non_negative_number(scope, "moving_window_size", &self.moving_window_size)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelCheckpointOverride {
    Percentage {
        threshold_percent: u8,
    },
    Custom {
        token_threshold: u32,
        max_token_limit: u32,
        max_output_tokens: u32,
    },
}

impl ModelCheckpointOverride {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match *self {
            Self::Percentage { threshold_percent } => {
                if !(1..=100).contains(&threshold_percent) {
                    return Err("checkpoint threshold_percent must be between 1 and 100");
                }
            }
            Self::Custom {
                token_threshold,
                max_token_limit,
                max_output_tokens,
            } => validate_absolute_limits(
                "model checkpoint override",
                token_threshold,
                max_token_limit,
                max_output_tokens,
            )
            .map_err(|_| "model checkpoint override limits are invalid")?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClaudeCheckpointMetadata {
    pub capacity: u32,
    pub output_token_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EffectiveCheckpointLimits {
    pub(crate) token_threshold: u32,
    pub(crate) max_token_limit: u32,
    pub(crate) max_output_tokens: u32,
}

impl EffectiveCheckpointLimits {
    const fn new(token_threshold: u32, max_token_limit: u32, max_output_tokens: u32) -> Self {
        Self {
            token_threshold,
            max_token_limit,
            max_output_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialModelSettings {
    pub gemini: CompressionLimitsPolicy,
    pub claude: CompressionLimitsPolicy,
    pub custom_model: CompressionLimitsPolicy,
    pub custom_model_checkpoint: CheckpointExecutionPolicy,
    pub model_checkpoint_policies: BTreeMap<String, CheckpointExecutionPolicy>,
}

impl Default for OfficialModelSettings {
    fn default() -> Self {
        Self {
            gemini: CompressionLimitsPolicy::percentage(false),
            claude: CompressionLimitsPolicy::percentage(false),
            custom_model: CompressionLimitsPolicy::percentage(true),
            custom_model_checkpoint: CheckpointExecutionPolicy::default(),
            model_checkpoint_policies: BTreeMap::new(),
        }
    }
}

impl OfficialModelSettings {
    pub(crate) fn custom_model_checkpoint_policy(
        &self,
        upstream_model_id: &str,
    ) -> &CheckpointExecutionPolicy {
        self.model_checkpoint_policies
            .get(upstream_model_id)
            .unwrap_or(&self.custom_model_checkpoint)
    }

    pub(crate) fn gemini_checkpoint_limits(&self) -> Option<EffectiveCheckpointLimits> {
        self.gemini
            .enabled
            .then(|| limits_from_policy(&self.gemini, GEMINI_CONTEXT_WINDOW_LIMIT, None))
            .and_then(valid_effective_limits)
    }

    pub(crate) fn claude_checkpoint_limits(
        &self,
        metadata: ClaudeCheckpointMetadata,
    ) -> Option<EffectiveCheckpointLimits> {
        if !self.claude.enabled || metadata.capacity == 0 {
            return None;
        }
        let limits =
            limits_from_policy(&self.claude, metadata.capacity, metadata.output_token_limit);
        valid_effective_limits(limits)
    }

    pub(crate) fn custom_model_checkpoint_limits_with_override(
        &self,
        checkpoint_override: Option<&ModelCheckpointOverride>,
        effective_token_limit: u32,
        output_token_limit: u32,
    ) -> Option<EffectiveCheckpointLimits> {
        let limits = match checkpoint_override {
            Some(ModelCheckpointOverride::Custom {
                token_threshold,
                max_token_limit,
                max_output_tokens,
            }) => EffectiveCheckpointLimits::new(
                *token_threshold,
                *max_token_limit,
                *max_output_tokens,
            ),
            Some(ModelCheckpointOverride::Percentage { threshold_percent }) => {
                let base = limits_from_policy(
                    &self.custom_model,
                    effective_token_limit,
                    Some(output_token_limit),
                );
                EffectiveCheckpointLimits::new(
                    (u64::from(base.max_token_limit) * u64::from(*threshold_percent)).div_ceil(100)
                        as u32,
                    base.max_token_limit,
                    base.max_output_tokens,
                )
            }
            None => limits_from_policy(
                &self.custom_model,
                effective_token_limit,
                Some(output_token_limit),
            ),
        };
        valid_effective_limits(clamp_limits(
            limits,
            effective_token_limit,
            output_token_limit,
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.gemini.validate("gemini")?;
        self.claude.validate("claude")?;
        self.custom_model.validate("custom_model")?;
        if !self.custom_model.enabled {
            return Err("custom_model must be enabled".to_string());
        }
        self.custom_model_checkpoint
            .validate("custom_model_checkpoint")?;
        if !self.custom_model_checkpoint.enabled {
            return Err("custom_model_checkpoint must be enabled".to_string());
        }
        for (model_id, policy) in &self.model_checkpoint_policies {
            if model_id.trim().is_empty() {
                return Err(
                    "model_checkpoint_policies cannot contain an empty model ID".to_string()
                );
            }
            policy.validate(&format!("model_checkpoint_policies[{model_id}]"))?;
            if !policy.enabled {
                return Err(format!(
                    "model_checkpoint_policies[{model_id}] must be enabled"
                ));
            }
        }
        Ok(())
    }
}

fn validate_absolute_limits(
    scope: &str,
    token_threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) -> Result<(), String> {
    if token_threshold == 0 || max_token_limit == 0 || max_output_tokens == 0 {
        return Err(format!("{scope} limits must be greater than 0"));
    }
    if token_threshold >= max_token_limit {
        return Err(format!(
            "{scope} token_threshold must be less than max_token_limit"
        ));
    }
    if max_output_tokens >= max_token_limit {
        return Err(format!(
            "{scope} max_output_tokens must be less than max_token_limit"
        ));
    }
    if u64::from(token_threshold) + u64::from(max_output_tokens) > u64::from(max_token_limit) {
        return Err(format!(
            "{scope} threshold plus output reserve exceeds max_token_limit"
        ));
    }
    Ok(())
}

fn validate_non_negative_number(scope: &str, name: &str, value: &str) -> Result<(), String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("{scope} {name} must be numeric"))?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(format!("{scope} {name} must be finite and non-negative"));
    }
    Ok(())
}

fn limits_from_policy(
    policy: &CompressionLimitsPolicy,
    capacity: u32,
    output_token_limit: Option<u32>,
) -> EffectiveCheckpointLimits {
    let limits = match policy.mode {
        CheckpointLimitMode::Percentage => EffectiveCheckpointLimits::new(
            scale_percent(capacity, policy.token_threshold_percent),
            scale_percent(capacity, policy.max_token_limit_percent),
            scale_percent(capacity, policy.max_output_tokens_percent),
        ),
        CheckpointLimitMode::Absolute => EffectiveCheckpointLimits::new(
            policy.token_threshold,
            policy.max_token_limit,
            policy.max_output_tokens,
        ),
    };
    clamp_limits(limits, capacity, output_token_limit.unwrap_or(u32::MAX))
}

fn clamp_limits(
    limits: EffectiveCheckpointLimits,
    capacity: u32,
    output_token_limit: u32,
) -> EffectiveCheckpointLimits {
    let max_token_limit = limits.max_token_limit.max(2).min(capacity);
    let max_output_tokens = limits
        .max_output_tokens
        .min(output_token_limit)
        .min(max_token_limit.saturating_sub(1));
    let token_threshold = limits
        .token_threshold
        .min(max_token_limit.saturating_sub(max_output_tokens));
    EffectiveCheckpointLimits::new(token_threshold, max_token_limit, max_output_tokens)
}

fn valid_effective_limits(limits: EffectiveCheckpointLimits) -> Option<EffectiveCheckpointLimits> {
    (limits.token_threshold > 0
        && limits.max_token_limit > 0
        && limits.max_output_tokens > 0
        && limits.token_threshold < limits.max_token_limit
        && u64::from(limits.token_threshold) + u64::from(limits.max_output_tokens)
            <= u64::from(limits.max_token_limit))
    .then_some(limits)
}

fn scale_percent(value: u32, percent: u8) -> u32 {
    let scaled = u64::from(value) * u64::from(percent);
    scaled.div_ceil(100) as u32
}

#[cfg(test)]
mod tests;
