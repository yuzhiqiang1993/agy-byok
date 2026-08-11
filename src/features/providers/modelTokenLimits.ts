import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelTokenLimits } from "../../types/config";

type TokenLimitPresetId =
  | "estimated_default"
  | "chatgpt_default"
  | "chatgpt_thinking"
  | "gpt5_api"
  | "gemini_long"
  | "claude_long"
  | "custom";

interface TokenLimitPreset {
  id: Exclude<TokenLimitPresetId, "custom">;
  input_token_limit: number;
  output_token_limit: number;
}

export const DEFAULT_TOKEN_LIMIT = 128_000;
export const DEFAULT_INPUT_TOKEN_LIMIT = 128_000;
export const DEFAULT_OUTPUT_TOKEN_LIMIT = 65_536;
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

export const TOKEN_LIMIT_PRESETS: readonly TokenLimitPreset[] = [
  {
    id: "estimated_default",
    input_token_limit: DEFAULT_INPUT_TOKEN_LIMIT,
    output_token_limit: DEFAULT_OUTPUT_TOKEN_LIMIT,
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
];

export function tokenLimitsForPreset(id: string): ModelTokenLimits | null {
  const preset = TOKEN_LIMIT_PRESETS.find((item) => item.id === id);
  return preset
    ? {
        context_window: null,
        context_window_source: "unknown",
        input_token_limit: preset.input_token_limit,
        input_token_limit_source: "configured",
        output_token_limit: preset.output_token_limit,
        output_token_limit_source: "configured",
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
  const contextWindow = catalogContextWindow(model);
  return {
    context_window: contextWindow ?? existing?.context_window ?? DEFAULT_CONTEXT_WINDOW,
    context_window_source: tokenLimitSource(
      contextWindow,
      existing?.context_window,
      existing?.context_window_source,
    ),
    input_token_limit: model.inputTokenLimit ?? existing?.input_token_limit ?? DEFAULT_INPUT_TOKEN_LIMIT,
    input_token_limit_source: tokenLimitSource(
      model.inputTokenLimit,
      existing?.input_token_limit,
      existing?.input_token_limit_source,
    ),
    output_token_limit: model.outputTokenLimit ?? existing?.output_token_limit ?? DEFAULT_OUTPUT_TOKEN_LIMIT,
    output_token_limit_source: tokenLimitSource(
      model.outputTokenLimit,
      existing?.output_token_limit,
      existing?.output_token_limit_source,
    ),
  };
}

function tokenLimitSource(
  catalogValue: number | undefined,
  existingValue: number | null | undefined,
  existingSource: ModelTokenLimits["context_window_source"] | undefined,
): ModelTokenLimits["context_window_source"] {
  if (catalogValue !== undefined) return "catalog";
  if (existingValue !== null && existingValue !== undefined) return existingSource ?? "unknown";
  return "estimated";
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
