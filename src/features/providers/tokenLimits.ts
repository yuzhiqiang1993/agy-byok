import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  ModelCheckpointOverride,
  ModelTokenLimits,
  OfficialModelSettings,
} from "../../types/config";

export type TokenLimitPresetId =
  | "estimated_default"
  | "chatgpt_default"
  | "chatgpt_thinking"
  | "gpt5_api"
  | "gemini_long"
  | "claude_long"
  | "compatibility"
  | "custom";

export interface TokenLimitPreset {
  id: Exclude<TokenLimitPresetId, "custom">;
  input_token_limit: number;
  output_token_limit: number;
}

export const DEFAULT_TOKEN_LIMIT = 128_000;
export const DEFAULT_CONTEXT_WINDOW = 128_000;

export const CONTEXT_WINDOW_OPTIONS = [
  128_000,
  200_000,
  256_000,
  500_000,
  1_000_000,
  1_048_576,
] as const;

export const TOKEN_INPUT_LIMIT_OPTIONS = [
  128_000,
  256_000,
  400_000,
  512_000,
  1_000_000,
  1_050_000,
] as const;

export const TOKEN_OUTPUT_LIMIT_OPTIONS = [
  8_192,
  16_384,
  32_768,
  65_536,
  128_000,
] as const;

export interface CustomModelCheckpointLimits {
  threshold: number;
  max_token_limit: number;
  max_output_tokens: number;
  threshold_percent: string;
  clipped: boolean;
}

const MAX_U32 = 0xffffffff;
const COMPRESSION_PROFILE_REFERENCE_LIMIT = 1_048_576;

function isPositiveInteger(value: number): boolean {
  return Number.isInteger(value) && value > 0 && value <= MAX_U32;
}

function isValidPercentage(value: number): boolean {
  return Number.isInteger(value) && value >= 1 && value <= 100;
}

export function isValidModelCheckpointOverride(
  override: ModelCheckpointOverride | null,
): boolean {
  if (override === null) return true;
  if (override.kind === "percentage") {
    return isValidPercentage(override.threshold_percent);
  }
  return isPositiveInteger(override.token_threshold)
    && isPositiveInteger(override.max_token_limit)
    && isPositiveInteger(override.max_output_tokens)
    && override.token_threshold + override.max_output_tokens <= override.max_token_limit;
}

export function customModelCheckpointLimits(
  settings: Partial<OfficialModelSettings> | undefined,
  limits: ModelTokenLimits | undefined,
  override: ModelCheckpointOverride | null,
): CustomModelCheckpointLimits | null {
  if (!isValidModelCheckpointOverride(override)) return null;

  const contextLimit = limits?.context_window ?? DEFAULT_CONTEXT_WINDOW;
  const inputLimit = limits?.input_token_limit ?? DEFAULT_TOKEN_LIMIT;
  const outputLimit = limits?.output_token_limit ?? DEFAULT_TOKEN_LIMIT;
  const checkpointTokenLimit = Math.min(contextLimit, inputLimit);
  const profile = settings?.custom_model_compression_profile ?? "balanced";
  const referenceValues = profile === "safe"
    ? { threshold: 430_000, maxTokenLimit: 512_000 }
    : profile === "aggressive"
      ? { threshold: 760_000, maxTokenLimit: 900_000 }
      : { threshold: 640_000, maxTokenLimit: 768_000 };
  const scaleReferenceValue = (value: number): number => Math.floor(
    checkpointTokenLimit * value / COMPRESSION_PROFILE_REFERENCE_LIMIT,
  );
  const customPercentThreshold = settings?.custom_model_token_threshold_percent;
  const customPercentHardLimit = settings?.custom_model_max_token_limit_percent;
  const customPercentOutputReserve = settings?.custom_model_max_output_tokens_percent;
  const customProfileIsValid = isValidPercentage(customPercentThreshold ?? 0)
    && isValidPercentage(customPercentHardLimit ?? 0)
    && isValidPercentage(customPercentOutputReserve ?? 0);
  if (profile === "custom" && !customProfileIsValid && override?.kind !== "custom") {
    return null;
  }
  const profileValues = profile === "custom"
    ? {
        threshold: Math.floor(checkpointTokenLimit * (customPercentThreshold ?? 0) / 100),
        maxTokenLimit: Math.floor(checkpointTokenLimit * (customPercentHardLimit ?? 0) / 100),
      }
    : {
        threshold: scaleReferenceValue(referenceValues.threshold),
        maxTokenLimit: scaleReferenceValue(referenceValues.maxTokenLimit),
      };
  const profileOutputLimit = profile === "custom"
    ? Math.floor(checkpointTokenLimit * (customPercentOutputReserve ?? 0) / 100)
    : scaleReferenceValue(16_384);
  const requestedThreshold = override?.kind === "custom"
    ? override.token_threshold
    : profileValues.threshold;
  const requestedMaxTokenLimit = override?.kind === "custom"
    ? override.max_token_limit
    : profileValues.maxTokenLimit;
  const requestedMaxOutputTokens = override?.kind === "custom"
    ? override.max_output_tokens
    : profileOutputLimit;
  const thresholdPercent = override?.kind === "percentage"
    ? override.threshold_percent
    : null;
  const maxTokenLimit = Math.min(requestedMaxTokenLimit, checkpointTokenLimit);
  const maxOutputTokens = Math.min(
    requestedMaxOutputTokens,
    outputLimit,
    Math.max(maxTokenLimit - 1, 0),
  );
  const thresholdBeforeReserve = thresholdPercent !== null
    ? Math.floor(maxTokenLimit * thresholdPercent / 100)
    : requestedThreshold;
  const threshold = Math.min(
    thresholdBeforeReserve,
    Math.max(maxTokenLimit - maxOutputTokens, 0),
  );
  if (threshold <= 0 || maxOutputTokens <= 0 || threshold >= maxTokenLimit) return null;
  return {
    threshold,
    max_token_limit: maxTokenLimit,
    max_output_tokens: maxOutputTokens,
    threshold_percent: `${Math.round((threshold / maxTokenLimit) * 1000) / 10}%`,
    clipped: maxTokenLimit !== requestedMaxTokenLimit
      || maxOutputTokens !== requestedMaxOutputTokens
      || threshold !== thresholdBeforeReserve,
  };
}

export const TOKEN_LIMIT_PRESETS: readonly TokenLimitPreset[] = [
  {
    id: "estimated_default",
    input_token_limit: DEFAULT_TOKEN_LIMIT,
    output_token_limit: DEFAULT_TOKEN_LIMIT,
  },
  {
    id: "chatgpt_default",
    input_token_limit: 400_000,
    output_token_limit: 128_000,
  },
  {
    id: "chatgpt_thinking",
    input_token_limit: 256_000,
    output_token_limit: 128_000,
  },
  {
    id: "gpt5_api",
    input_token_limit: 1_050_000,
    output_token_limit: 128_000,
  },
  {
    id: "gemini_long",
    input_token_limit: 1_000_000,
    output_token_limit: 65_536,
  },
  {
    id: "claude_long",
    input_token_limit: 1_000_000,
    output_token_limit: 128_000,
  },
  {
    id: "compatibility",
    input_token_limit: 128_000,
    output_token_limit: 8_192,
  },
];

export function tokenLimitsForPreset(id: string): ModelTokenLimits | null {
  const preset = TOKEN_LIMIT_PRESETS.find((item) => item.id === id);
  return preset
    ? {
        context_window: null,
        input_token_limit: preset.input_token_limit,
        output_token_limit: preset.output_token_limit,
      }
    : null;
}

export function catalogContextWindow(model: ProviderCatalogModel): number | undefined {
  return model.contextWindow ?? model.contextLength ?? model.maxContextWindow;
}

export function resolveCatalogTokenLimits(
  model: ProviderCatalogModel,
  existing?: ModelTokenLimits,
): ModelTokenLimits {
  return {
    // 目录是本次同步得到的事实；目录缺失时沿用已保存值，否则使用保守的经验默认值。
    context_window: catalogContextWindow(model) ?? existing?.context_window ?? DEFAULT_CONTEXT_WINDOW,
    input_token_limit: model.inputTokenLimit ?? existing?.input_token_limit ?? DEFAULT_TOKEN_LIMIT,
    output_token_limit: model.outputTokenLimit ?? existing?.output_token_limit ?? DEFAULT_TOKEN_LIMIT,
  };
}

export function presetIdForTokenLimits(limits: ModelTokenLimits): TokenLimitPresetId {
  const preset = TOKEN_LIMIT_PRESETS.find(
    (item) =>
      item.input_token_limit === limits.input_token_limit
      && item.output_token_limit === limits.output_token_limit,
  );
  return preset?.id ?? "custom";
}

export function formatTokenLimit(value: number | null): string {
  if (value === null) return "—";
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
