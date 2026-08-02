import type { ReasoningLevel, ConfigurableReasoningLevel, ReasoningMapping } from "../types/reasoning";
import type { ProviderProtocol, UpstreamModel, VirtualModel } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";

export const REASONING_LEVEL_ORDER: Record<ReasoningLevel, number> = {
  off: 0,
  low: 1,
  medium: 2,
  high: 3,
  x_high: 4,
  max: 5,
  auto: 6,
};

export function sortReasoningLevels<T extends ReasoningLevel>(levels: Iterable<T>): T[] {
  return [...levels].sort(
    (a, b) => (REASONING_LEVEL_ORDER[a] ?? 99) - (REASONING_LEVEL_ORDER[b] ?? 99),
  );
}

export function sortVirtualModelsByReasoningLevel(virtualModels: VirtualModel[]): VirtualModel[] {
  return [...virtualModels].sort((a, b) => {
    const orderA = a.default_reasoning_level ? (REASONING_LEVEL_ORDER[a.default_reasoning_level] ?? 99) : -1;
    const orderB = b.default_reasoning_level ? (REASONING_LEVEL_ORDER[b.default_reasoning_level] ?? 99) : -1;
    return orderA - orderB;
  });
}

export function reasoningLevelLabel(level: ReasoningLevel): string {
  return {
    off: "Off",
    low: "Low",
    medium: "Medium",
    high: "High",
    x_high: "Extra High",
    max: "Max",
    auto: "自定义",
  }[level];
}

export function configurableReasoningLevels(protocol: ProviderProtocol): ConfigurableReasoningLevel[] {
  return protocol === "gemini_generate_content"
    ? ["low", "medium", "high"]
    : ["low", "medium", "high", "x_high", "max"];
}

export function catalogReasoningLevelsForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  existingUpstream?: UpstreamModel,
): ConfigurableReasoningLevel[] {
  if (model.reasoning?.supported === false && !existingUpstream) return [];
  const explicit = (model.reasoning?.levels ?? []).filter(
    (level): level is ConfigurableReasoningLevel =>
      configurableReasoningLevels(protocol).includes(level as ConfigurableReasoningLevel),
  );
  if (explicit.length > 0) return sortReasoningLevels([...new Set(explicit)]);
  const existing = existingUpstream
    ? (Object.keys(existingUpstream.capabilities.reasoning.levels) as ReasoningLevel[]).filter(
        (level): level is ConfigurableReasoningLevel =>
          configurableReasoningLevels(protocol).includes(level as ConfigurableReasoningLevel),
      )
    : [];
  return sortReasoningLevels(
    existing.length > 0 ? [...new Set(existing)] : configurableReasoningLevels(protocol),
  );
}

export function catalogReasoningMetadataLabel(model: ProviderCatalogModel): string | null {
  const metadata = model.reasoning;
  if (!metadata) return null;
  if (metadata.supported === false) return "思考：不支持";
  const levels = (metadata.levels ?? []).filter((level) => level !== "off" && level !== "auto");
  if (levels.length > 0) return `思考：${sortReasoningLevels(levels).map(reasoningLevelLabel).join(" · ")}`;
  if (metadata.supported === true) return "思考：支持（等级未声明）";
  return "思考：未声明";
}

export function reasoningLevels(protocol: ProviderProtocol): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  if (protocol === "anthropic_messages") {
    return {
      low: { kind: "budget_tokens", value: 1024 },
      medium: { kind: "budget_tokens", value: 4096 },
      high: { kind: "budget_tokens", value: 8192 },
      x_high: { kind: "budget_tokens", value: 16384 },
      max: { kind: "budget_tokens", value: 32768 },
    };
  }
  if (protocol === "gemini_generate_content") {
    return {
      low: { kind: "native_level", value: "low" },
      medium: { kind: "native_level", value: "medium" },
      high: { kind: "native_level", value: "high" },
    };
  }
  return {
    low: { kind: "effort", value: "low" },
    medium: { kind: "effort", value: "medium" },
    high: { kind: "effort", value: "high" },
    x_high: { kind: "effort", value: "xhigh" },
    max: { kind: "effort", value: "max" },
  };
}

export function customReasoningMapping(protocol: ProviderProtocol, value: string): ReasoningMapping | null {
  const normalized = value.trim();
  if (!normalized) return null;
  if (protocol === "openai_chat_completions" || protocol === "openai_responses") {
    return { kind: "effort", value: normalized };
  }
  const budgetTokens = Number(normalized);
  if (!Number.isInteger(budgetTokens) || budgetTokens < 1024) return null;
  return { kind: "budget_tokens", value: budgetTokens };
}

export function reasoningLevelsForVirtualModels(
  protocol: ProviderProtocol,
  virtualModels: VirtualModel[],
): Set<ConfigurableReasoningLevel> {
  const configurable = new Set<ReasoningLevel>(configurableReasoningLevels(protocol));
  const levels = virtualModels.flatMap((virtualModel) => {
    const level = virtualModel.default_reasoning_level;
    return level && configurable.has(level)
      ? [level as ConfigurableReasoningLevel]
      : [];
  });
  return new Set(sortReasoningLevels(levels));
}

export function customReasoningValueFromUpstream(upstream: UpstreamModel): string | null {
  const mapping = upstream.capabilities.reasoning.levels.auto;
  if (!mapping) return null;
  if (mapping.kind === "effort" || mapping.kind === "native_level") return mapping.value;
  if (mapping.kind === "budget_tokens") return String(mapping.value);
  return null;
}
