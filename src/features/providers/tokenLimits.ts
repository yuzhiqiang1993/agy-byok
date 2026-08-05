import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelTokenLimits } from "../../types/config";

export type TokenLimitPresetId =
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

export const TOKEN_LIMIT_PRESETS: readonly TokenLimitPreset[] = [
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
        input_token_limit: preset.input_token_limit,
        output_token_limit: preset.output_token_limit,
      }
    : null;
}

export function resolveCatalogTokenLimits(
  model: ProviderCatalogModel,
  existing?: ModelTokenLimits,
): ModelTokenLimits {
  return {
    // 目录是本次同步得到的事实；只有目录没有返回某个字段时才沿用已保存值。
    input_token_limit: model.inputTokenLimit ?? existing?.input_token_limit ?? null,
    output_token_limit: model.outputTokenLimit ?? existing?.output_token_limit ?? null,
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
