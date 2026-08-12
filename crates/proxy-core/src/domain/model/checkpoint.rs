use serde::{Deserialize, Serialize};

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
pub struct ModelCompressionPolicy {
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
    pub token_threshold: u32,
    pub max_token_limit: u32,
    pub max_output_tokens: u32,
}

impl Default for ModelCompressionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            checkpoint_model: "MODEL_PLACEHOLDER_M71".to_string(),
            strategy: "CHECKPOINT_STRATEGY_UNSPECIFIED".to_string(),
            max_overhead_ratio: "0.30".to_string(),
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
            token_threshold: 50_000,
            max_token_limit: 128_000,
            max_output_tokens: 16_384,
        }
    }
}

impl ModelCompressionPolicy {
    pub fn resolve_effective(
        &self,
        capacity: Option<u32>,
        output_token_limit: Option<u32>,
    ) -> Option<Self> {
        let capacity = capacity.unwrap_or(self.max_token_limit);
        let output_token_limit = output_token_limit.unwrap_or(self.max_output_tokens);
        if capacity < 2 || output_token_limit == 0 {
            return None;
        }

        let max_token_limit = self.max_token_limit.min(capacity);
        if max_token_limit < 2 {
            return None;
        }
        let max_output_tokens = self
            .max_output_tokens
            .min(output_token_limit)
            .min(max_token_limit.saturating_sub(1));
        if max_output_tokens == 0 {
            return None;
        }
        let token_threshold = self
            .token_threshold
            .min(max_token_limit.saturating_sub(max_output_tokens));
        if token_threshold == 0 {
            return None;
        }

        let mut resolved = self.clone();
        resolved.token_threshold = token_threshold;
        resolved.max_token_limit = max_token_limit;
        resolved.max_output_tokens = max_output_tokens;
        Some(resolved)
    }

    pub fn validate(&self, scope: &str) -> Result<(), String> {
        if !is_valid_checkpoint_model(&self.checkpoint_model) {
            return Err(format!(
                "{scope} checkpoint_model must match MODEL_PLACEHOLDER_M<number>"
            ));
        }
        if self.strategy.trim().is_empty() {
            return Err(format!("{scope} strategy cannot be empty"));
        }
        validate_non_negative_number(scope, "max_overhead_ratio", &self.max_overhead_ratio)?;
        validate_non_negative_number(scope, "moving_window_size", &self.moving_window_size)?;
        validate_token_limits(
            scope,
            self.token_threshold,
            self.max_token_limit,
            self.max_output_tokens,
        )
    }
}

fn is_valid_checkpoint_model(value: &str) -> bool {
    // UI 仍只提供已验证的 Worker，但官方目录中的新 placeholder 必须能够原样继承。
    value
        .strip_prefix("MODEL_PLACEHOLDER_M")
        .is_some_and(|number| !number.is_empty() && number.parse::<u32>().is_ok())
}

fn validate_token_limits(
    scope: &str,
    token_threshold: u32,
    max_token_limit: u32,
    max_output_tokens: u32,
) -> Result<(), String> {
    if token_threshold == 0 || max_token_limit == 0 || max_output_tokens == 0 {
        return Err(format!("{scope} token limits must be greater than 0"));
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
            "{scope} token_threshold plus max_output_tokens must not exceed max_token_limit"
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

#[cfg(test)]
mod tests;
