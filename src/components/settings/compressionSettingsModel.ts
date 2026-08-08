import { createDefaultOfficialModelSettings } from "../../config/defaults";
import type {
  CheckpointExecutionPolicy,
  CheckpointLimitMode,
  CompressionLimitsPolicy,
  OfficialModelSettings,
} from "../../types/config";

export type CompressionScope = "gemini" | "claude" | "custom_model";

const LIMIT_POLICY_KEYS = [
  "enabled",
  "mode",
  "token_threshold_percent",
  "max_token_limit_percent",
  "max_output_tokens_percent",
  "token_threshold",
  "max_token_limit",
  "max_output_tokens",
] as const;

const EXECUTION_POLICY_KEYS = [
  "enabled",
  "checkpoint_model",
  "strategy",
  "max_overhead_ratio",
  "moving_window_size",
  "use_last_planner_model",
  "is_sync",
  "max_user_requests",
  "include_last_user_message",
  "include_conversation_log",
  "include_running_task_snapshots",
  "include_subagent_snapshots",
  "include_artifact_snapshots",
  "retry_config",
] as const;

const RETRY_CONFIG_KEYS = [
  "max_retries",
  "initial_sleep_duration_ms",
  "exponential_multiplier",
  "include_error_feedback",
] as const;

const OFFICIAL_SETTINGS_KEYS = [
  "gemini",
  "claude",
  "custom_model",
  "custom_model_checkpoint",
  "model_checkpoint_policies",
] as const;

const MAX_U32 = 0xffffffff;

export const CHECKPOINT_EXECUTOR_IDS = [
  "MODEL_PLACEHOLDER_M50",
  "MODEL_PLACEHOLDER_M71",
  "MODEL_PLACEHOLDER_M72",
] as const;

export const DEFAULT_COMPRESSION_SETTINGS = createDefaultOfficialModelSettings();

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const expected = new Set(keys);
  const actual = Object.keys(value);
  return actual.length === expected.size && actual.every((key) => expected.has(key));
}

function isBoolean(value: unknown): value is boolean {
  return typeof value === "boolean";
}

function isIntegerInRange(value: unknown, min: number, max: number): value is number {
  return typeof value === "number"
    && Number.isInteger(value)
    && value >= min
    && value <= max;
}

function isU32(value: unknown): value is number {
  return isIntegerInRange(value, 0, MAX_U32);
}

function isLimitMode(value: unknown): value is CheckpointLimitMode {
  return value === "percentage" || value === "absolute";
}

function isNonNegativeNumericString(value: unknown): value is string {
  if (typeof value !== "string" || value.trim().length === 0) return false;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0;
}

export function compressionLimitsAreValid(value: unknown): value is CompressionLimitsPolicy {
  if (!isRecord(value) || !hasExactKeys(value, LIMIT_POLICY_KEYS)) return false;
  if (!isBoolean(value.enabled) || !isLimitMode(value.mode)) return false;
  if (!isIntegerInRange(value.token_threshold_percent, 0, 100)
    || !isIntegerInRange(value.max_token_limit_percent, 0, 100)
    || !isIntegerInRange(value.max_output_tokens_percent, 0, 100)
    || !isU32(value.token_threshold)
    || !isU32(value.max_token_limit)
    || !isU32(value.max_output_tokens)) {
    return false;
  }

  if (value.mode === "percentage") {
    return isIntegerInRange(value.token_threshold_percent, 1, 100)
      && isIntegerInRange(value.max_token_limit_percent, 1, 100)
      && isIntegerInRange(value.max_output_tokens_percent, 1, 100)
      && value.token_threshold_percent < value.max_token_limit_percent
      && value.max_output_tokens_percent < value.max_token_limit_percent
      && value.token_threshold_percent + value.max_output_tokens_percent <= value.max_token_limit_percent;
  }

  return value.token_threshold > 0
    && value.max_token_limit > 0
    && value.max_output_tokens > 0
    && value.token_threshold < value.max_token_limit
    && value.max_output_tokens < value.max_token_limit
    && value.token_threshold + value.max_output_tokens <= value.max_token_limit;
}

function retryConfigIsValid(value: unknown): boolean {
  if (!isRecord(value) || !hasExactKeys(value, RETRY_CONFIG_KEYS)) return false;
  return isU32(value.max_retries)
    && isU32(value.initial_sleep_duration_ms)
    && isU32(value.exponential_multiplier)
    && isBoolean(value.include_error_feedback);
}

export function checkpointExecutionPolicyIsValid(value: unknown): value is CheckpointExecutionPolicy {
  if (!isRecord(value) || !hasExactKeys(value, EXECUTION_POLICY_KEYS)) return false;
  return isBoolean(value.enabled)
    && typeof value.checkpoint_model === "string"
    && CHECKPOINT_EXECUTOR_IDS.some((modelId) => modelId === value.checkpoint_model)
    && typeof value.strategy === "string"
    && value.strategy.trim().length > 0
    && isNonNegativeNumericString(value.max_overhead_ratio)
    && isNonNegativeNumericString(value.moving_window_size)
    && isBoolean(value.use_last_planner_model)
    && isBoolean(value.is_sync)
    && isU32(value.max_user_requests)
    && isBoolean(value.include_last_user_message)
    && isBoolean(value.include_conversation_log)
    && isBoolean(value.include_running_task_snapshots)
    && isBoolean(value.include_subagent_snapshots)
    && isBoolean(value.include_artifact_snapshots)
    && retryConfigIsValid(value.retry_config);
}

export function compressionSettingsAreValid(value: unknown): value is OfficialModelSettings {
  if (!isRecord(value) || !hasExactKeys(value, OFFICIAL_SETTINGS_KEYS)) return false;
  if (!compressionLimitsAreValid(value.gemini)
    || !compressionLimitsAreValid(value.claude)
    || !compressionLimitsAreValid(value.custom_model)
    || !checkpointExecutionPolicyIsValid(value.custom_model_checkpoint)
    || !isRecord(value.model_checkpoint_policies)
    || !value.custom_model.enabled
    || !value.custom_model_checkpoint.enabled) {
    return false;
  }
  return Object.entries(value.model_checkpoint_policies).every(([modelId, policy]) =>
    modelId.trim().length > 0
      && checkpointExecutionPolicyIsValid(policy)
      && (policy as CheckpointExecutionPolicy).enabled);
}

export function cloneExecutionPolicy(value: CheckpointExecutionPolicy): CheckpointExecutionPolicy {
  return {
    ...value,
    retry_config: { ...value.retry_config },
  };
}

export function cloneCompressionSettings(value: OfficialModelSettings): OfficialModelSettings {
  return {
    gemini: { ...value.gemini },
    claude: { ...value.claude },
    custom_model: { ...value.custom_model },
    custom_model_checkpoint: cloneExecutionPolicy(value.custom_model_checkpoint),
    model_checkpoint_policies: Object.fromEntries(
      Object.entries(value.model_checkpoint_policies).map(([key, policy]) => [key, cloneExecutionPolicy(policy)]),
    ),
  };
}

export function compressionSettingsAreEqual(
  left: OfficialModelSettings,
  right: OfficialModelSettings,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function updateCompressionLimits(
  settings: OfficialModelSettings,
  scope: CompressionScope,
  patch: Partial<CompressionLimitsPolicy>,
): OfficialModelSettings {
  return {
    ...settings,
    [scope]: { ...settings[scope], ...patch },
  };
}

function parseLimits(value: unknown): CompressionLimitsPolicy | null {
  return compressionLimitsAreValid(value)
    ? { ...(value as CompressionLimitsPolicy) }
    : null;
}

function parseExecutionPolicy(value: unknown): CheckpointExecutionPolicy | null {
  return checkpointExecutionPolicyIsValid(value)
    ? {
        ...(value as CheckpointExecutionPolicy),
        retry_config: { ...(value as CheckpointExecutionPolicy).retry_config },
      }
    : null;
}

export function parseCompressionSettings(value: unknown): OfficialModelSettings | null {
  if (!isRecord(value) || !hasExactKeys(value, OFFICIAL_SETTINGS_KEYS)) return null;
  const gemini = parseLimits(value.gemini);
  const claude = parseLimits(value.claude);
  const customModel = parseLimits(value.custom_model);
  const customModelCheckpoint = parseExecutionPolicy(value.custom_model_checkpoint);
  if (!gemini || !claude || !customModel || !customModelCheckpoint) return null;
  if (!isRecord(value.model_checkpoint_policies)) return null;

  const modelPolicies: Record<string, CheckpointExecutionPolicy> = {};
  for (const [modelId, policyValue] of Object.entries(value.model_checkpoint_policies)) {
    if (modelId.trim().length === 0) return null;
    const policy = parseExecutionPolicy(policyValue);
    if (!policy) return null;
    modelPolicies[modelId] = policy;
  }

  const result: OfficialModelSettings = {
    gemini,
    claude,
    custom_model: customModel,
    custom_model_checkpoint: customModelCheckpoint,
    model_checkpoint_policies: modelPolicies,
  };
  return compressionSettingsAreValid(result) ? result : null;
}
