import {
  DEFAULT_PROXY_PORT,
  type AppConfig,
  type CompressionLimitsPolicy,
  type CheckpointExecutionPolicy,
  type OfficialModelSettings,
} from "../types/config";

function createDefaultLimits(enabled: boolean): CompressionLimitsPolicy {
  return {
    enabled,
    mode: "percentage",
    token_threshold_percent: 61,
    max_token_limit_percent: 73,
    max_output_tokens_percent: 2,
    token_threshold: 0,
    max_token_limit: 0,
    max_output_tokens: 0,
  };
}

function createDefaultCustomCheckpointExecutionPolicy(): CheckpointExecutionPolicy {
  return {
    enabled: true,
    checkpoint_model: "MODEL_PLACEHOLDER_M71",
    strategy: "CHECKPOINT_STRATEGY_UNSPECIFIED",
    max_overhead_ratio: "0.15",
    moving_window_size: "1",
    use_last_planner_model: false,
    is_sync: false,
    max_user_requests: 10,
    include_last_user_message: false,
    include_conversation_log: true,
    include_running_task_snapshots: true,
    include_subagent_snapshots: true,
    include_artifact_snapshots: true,
    retry_config: {
      max_retries: 0,
      initial_sleep_duration_ms: 1000,
      exponential_multiplier: 2,
      include_error_feedback: false,
    },
  };
}

export function createDefaultOfficialModelSettings(): OfficialModelSettings {
  return {
    gemini: createDefaultLimits(false),
    claude: createDefaultLimits(false),
    custom_model: createDefaultLimits(true),
    custom_model_checkpoint: createDefaultCustomCheckpointExecutionPolicy(),
    model_checkpoint_policies: {},
  };
}

export function createDefaultAppConfig(): AppConfig {
  return {
    proxy_port: DEFAULT_PROXY_PORT,
    providers: [],
    upstream_models: [],
    virtual_models: [],
    official_model_settings: createDefaultOfficialModelSettings(),
  };
}
