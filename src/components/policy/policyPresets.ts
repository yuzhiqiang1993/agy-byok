import { t } from "../../i18n";
import type { UpstreamCompressionPolicy } from "../../types/catalog";
import type { ModelCompressionPolicy } from "../../types/config";

export type CompressionPresetId = "CONTEXT_256K" | "CONTEXT_372K" | "CONTEXT_500K" | "CONTEXT_1M";

export type PolicyMode = "NONE" | CompressionPresetId | "CUSTOM";

export type CompressionPolicyScope = "official_threshold_override" | "custom_full_policy";

export interface CompressionWorkerPolicy {
  checkpointModel: string;
  useLastPlannerModel: boolean;
  strategy: string;
}

export interface CompressionPreset {
  id: CompressionPresetId;
  labelKey:
    | "models.presetContext256K"
    | "models.presetContext372K"
    | "models.presetContext500K"
    | "models.presetContext1M";
  minCapacity: number;
  values: Pick<ModelCompressionPolicy, "token_threshold" | "max_token_limit" | "max_output_tokens">;
}

export const DEFAULT_OUTPUT_RESERVE = 16_384;
export const DEFAULT_MAX_OUTPUT_RESERVE = 65_536;

export const DEFAULT_CHECKPOINT_MODEL = "MODEL_PLACEHOLDER_M50";
export const WORKER_MODEL_PATTERN = /^MODEL_PLACEHOLDER_M\d+$/;

export const DEFAULT_POLICY_LIMITS = {
  token_threshold: 50_000,
  max_token_limit: 128_000,
  max_output_tokens: DEFAULT_OUTPUT_RESERVE,
};

export const COMPRESSION_PRESETS: readonly CompressionPreset[] = [
  {
    id: "CONTEXT_256K",
    labelKey: "models.presetContext256K",
    minCapacity: 256_000,
    values: { token_threshold: 102_400, max_token_limit: 153_600, max_output_tokens: 30_720 },
  },
  {
    id: "CONTEXT_372K",
    labelKey: "models.presetContext372K",
    minCapacity: 372_000,
    values: { token_threshold: 148_800, max_token_limit: 223_200, max_output_tokens: 44_640 },
  },
  {
    id: "CONTEXT_500K",
    labelKey: "models.presetContext500K",
    minCapacity: 500_000,
    values: { token_threshold: 200_000, max_token_limit: 300_000, max_output_tokens: 60_000 },
  },
  {
    id: "CONTEXT_1M",
    labelKey: "models.presetContext1M",
    minCapacity: 1_000_000,
    values: { token_threshold: 419_430, max_token_limit: 629_145, max_output_tokens: 65_535 },
  },
];

export const PRESET_IDS = COMPRESSION_PRESETS.map((preset) => preset.id);

export function presetById(id: CompressionPresetId): CompressionPreset {
  return COMPRESSION_PRESETS.find((preset) => preset.id === id) ?? COMPRESSION_PRESETS[0];
}

export function presetLabel(id: CompressionPresetId): string {
  return t(presetById(id).labelKey);
}

export function presetSupported(
  preset: CompressionPreset,
  capacity: number | null,
  outputTokenLimit: number | null,
): boolean {
  if (capacity == null || capacity < preset.minCapacity) return false;
  return outputTokenLimit == null || preset.values.max_output_tokens <= outputTokenLimit;
}

export function recommendedPresetForCapacity(
  capacity: number | null,
  outputTokenLimit?: number | null,
): CompressionPreset | null {
  if (capacity == null || capacity <= 0) return null;
  return (
    [...COMPRESSION_PRESETS]
      .reverse()
      .find((preset) => presetSupported(preset, capacity, outputTokenLimit ?? null)) ?? null
  );
}

export function createPolicy(
  limits: Pick<ModelCompressionPolicy, "token_threshold" | "max_token_limit" | "max_output_tokens">,
  worker: CompressionWorkerPolicy,
): ModelCompressionPolicy {
  return {
    enabled: true,
    checkpoint_model: worker.checkpointModel,
    strategy: worker.strategy,
    max_overhead_ratio: "0.30",
    moving_window_size: "1",
    use_last_planner_model: worker.useLastPlannerModel,
    is_sync: false,
    max_user_requests: 10,
    include_last_user_message: false,
    include_conversation_log: true,
    include_running_task_snapshots: true,
    include_subagent_snapshots: true,
    include_artifact_snapshots: true,
    retry_config: {
      max_retries: 0,
      initial_sleep_duration_ms: 1_000,
      exponential_multiplier: 2,
      include_error_feedback: false,
    },
    ...limits,
  };
}

export function clonePolicy(policy: ModelCompressionPolicy): ModelCompressionPolicy {
  return {
    ...policy,
    retry_config: { ...policy.retry_config },
  };
}

export function createPresetPolicy(
  id: CompressionPresetId,
  worker: CompressionWorkerPolicy,
  baseline?: ModelCompressionPolicy | null,
): ModelCompressionPolicy {
  const values = presetById(id).values;
  return baseline
    ? { ...clonePolicy(baseline), ...values }
    : createPolicy(values, worker);
}

export function matchingPreset(
  policy: ModelCompressionPolicy,
  capacity: number | null,
  outputTokenLimit: number | null,
): CompressionPresetId | null {
  if (!capacity || capacity <= 0) return null;
  const exact = COMPRESSION_PRESETS.find((preset) => (
    presetSupported(preset, capacity, outputTokenLimit)
      && policy.token_threshold === preset.values.token_threshold
      && policy.max_token_limit === preset.values.max_token_limit
      && policy.max_output_tokens === preset.values.max_output_tokens
  ))?.id;
  if (exact) return exact;
  return null;
}

export function initialMode(
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
  outputTokenLimit: number | null,
): PolicyMode {
  if (!policy) return "NONE";
  return matchingPreset(policy, capacity, outputTokenLimit) ?? "CUSTOM";
}

export function isValidWorkerModel(value: string | undefined): value is string {
  return value !== undefined && WORKER_MODEL_PATTERN.test(value);
}

export function workerPolicyFrom(policy: ModelCompressionPolicy): CompressionWorkerPolicy {
  return {
    checkpointModel: isValidWorkerModel(policy.checkpoint_model)
      ? policy.checkpoint_model
      : DEFAULT_CHECKPOINT_MODEL,
    useLastPlannerModel: policy.use_last_planner_model,
    strategy: policy.strategy || "CHECKPOINT_STRATEGY_UNSPECIFIED",
  };
}

export function defaultWorkerPolicy(upstream?: UpstreamCompressionPolicy): CompressionWorkerPolicy {
  return {
    checkpointModel: isValidWorkerModel(upstream?.checkpointModel)
      ? upstream.checkpointModel
      : DEFAULT_CHECKPOINT_MODEL,
    useLastPlannerModel: upstream?.useLastPlannerModel ?? false,
    strategy: upstream?.strategy || "CHECKPOINT_STRATEGY_UNSPECIFIED",
  };
}

export function formatTokenCount(value: number): string {
  if (value >= 1_000_000) {
    const millions = value / 1_000_000;
    return `${Number.isInteger(millions) ? millions : millions.toFixed(2).replace(/\.?0+$/, "")}M`;
  }
  if (value >= 1_000) {
    const thousands = value / 1_000;
    return `${Number.isInteger(thousands) ? thousands : thousands.toFixed(1).replace(/\.0$/, "")}K`;
  }
  return value.toLocaleString();
}

export function isValidPolicy(
  policy: ModelCompressionPolicy,
  capacity: number | null,
  outputTokenLimit: number | null,
): boolean {
  const { token_threshold: threshold, max_token_limit: limit, max_output_tokens: output } = policy;
  const maximumOutputReserve = outputTokenLimit ?? DEFAULT_MAX_OUTPUT_RESERVE;
  return [threshold, limit, output].every((value) => Number.isSafeInteger(value) && value > 0)
    && output >= DEFAULT_OUTPUT_RESERVE
    && output <= maximumOutputReserve
    && threshold < limit
    && output < limit
    && threshold + output <= limit
    && (capacity == null || limit <= capacity)
    && (outputTokenLimit == null || output <= outputTokenLimit);
}
