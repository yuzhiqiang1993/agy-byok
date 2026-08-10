import type { ReasoningLevel, ConfigurableReasoningLevel, ReasoningMapping } from "../types/reasoning";
import type { ProviderProtocol, UpstreamModel, VirtualModel } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";
import { t } from "../i18n";

const REASONING_LEVEL_ORDER: Record<ReasoningLevel, number> = {
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
    off: t("models.reasoningOff"),
    low: t("models.reasoningLow"),
    medium: t("models.reasoningMedium"),
    high: t("models.reasoningHigh"),
    x_high: t("models.reasoningExtraHigh"),
    max: t("models.reasoningMax"),
    auto: t("models.reasoningCustom"),
  }[level];
}

function configurableReasoningLevels(protocol: ProviderProtocol): ConfigurableReasoningLevel[] {
  return protocol === "gemini_generate_content"
    ? ["low", "medium", "high"]
    : ["low", "medium", "high", "x_high", "max"];
}

export function catalogReasoningIsAuthoritative(model: ProviderCatalogModel): boolean {
  return (model.reasoning?.levels ?? []).some((level) => level !== "off" && level !== "auto")
    || Object.keys(model.reasoning?.mappings ?? {}).some((level) => level !== "off" && level !== "auto");
}

export function catalogReasoningLevelsForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  existingUpstream?: UpstreamModel,
): ConfigurableReasoningLevel[] {
  const configurable = configurableReasoningLevels(protocol);
  const existing = existingUpstream
    ? (Object.keys(existingUpstream.capabilities.reasoning.levels) as ReasoningLevel[]).filter(
        (level): level is ConfigurableReasoningLevel => configurable.includes(level as ConfigurableReasoningLevel),
      )
    : [];

  if (model.reasoning?.supported === false) return sortReasoningLevels([...new Set(existing)]);

  const explicit = (model.reasoning?.levels ?? []).filter(
    (level): level is ConfigurableReasoningLevel =>
      configurable.includes(level as ConfigurableReasoningLevel),
  );
  if (explicit.length > 0) {
    return sortReasoningLevels([...new Set([...existing, ...explicit])]);
  }

  const mappings = catalogReasoningMappingsForModel(model, protocol);
  const mappedLevels = configurable.filter((level) => mappings[level] !== undefined);
  if (mappedLevels.length > 0) {
    return sortReasoningLevels([...new Set([...existing, ...mappedLevels])]);
  }

  if (existing.length > 0) return sortReasoningLevels([...new Set(existing)]);

  // 顶层预算是默认请求配置，不代表上游声明了 low/high 等离散档位。
  if (model.reasoning?.thinkingBudget != null || model.reasoning?.minThinkingBudget != null) {
    return [];
  }

  // 未声明具体等级时保留手动配置入口，避免把“目录未返回”误当成“不支持”。
  return configurable;
}

export function catalogReasoningMetadataLabel(model: ProviderCatalogModel): string | null {
  const metadata = model.reasoning;
  if (!metadata) return null;
  if (metadata.supported === false) return t("models.reasoningUnsupported");
  const levels = (metadata.levels ?? []).filter((level) => level !== "off" && level !== "auto");
  if (levels.length > 0) {
    return t("models.reasoningLevels", {
      levels: sortReasoningLevels(levels).map(reasoningLevelLabel).join(" · "),
    });
  }
  if (metadata.supported === true) return t("models.reasoningSupportedUndeclared");
  return t("models.reasoningUndeclared");
}

function defaultReasoningMapping(
  protocol: ProviderProtocol,
  level: ConfigurableReasoningLevel,
): ReasoningMapping | null {
  if (protocol === "anthropic_messages") {
    const budgetTokens = {
      low: 1_024,
      medium: 4_096,
      high: 8_192,
      x_high: 16_384,
      max: 32_768,
    }[level];
    return { kind: "budget_tokens", value: budgetTokens };
  }
  if (protocol === "gemini_generate_content") {
    return { kind: "native_level", value: level === "x_high" ? "xhigh" : level };
  }
  return { kind: "effort", value: level === "x_high" ? "xhigh" : level };
}

function reasoningLevels(
  protocol: ProviderProtocol,
): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  const levels = configurableReasoningLevels(protocol);
  return Object.fromEntries(
    levels.flatMap((level) => {
      const mapping = defaultReasoningMapping(protocol, level);
      return mapping ? [[level, mapping]] : [];
    }),
  ) as Partial<Record<ReasoningLevel, ReasoningMapping>>;
}

export function catalogReasoningMappingsForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  const mappings: Partial<Record<ReasoningLevel, ReasoningMapping>> = {
    ...(model.reasoning?.mappings ?? {}),
  };
  if (model.reasoning?.supported === false) return mappings;

  const defaults = reasoningLevels(protocol);
  const declaredLevels = (model.reasoning?.levels ?? []).filter(
    (level): level is ConfigurableReasoningLevel => configurableReasoningLevels(protocol).includes(level as ConfigurableReasoningLevel),
  );
  const levels = declaredLevels.length > 0
    ? declaredLevels
    : configurableReasoningLevels(protocol);
  for (const level of levels) {
    if (mappings[level] !== undefined) continue;
    const mapping = defaults[level as ConfigurableReasoningLevel];
    if (mapping) mappings[level] = mapping;
  }
  return mappings;
}

export function customReasoningMapping(protocol: ProviderProtocol, value: string): ReasoningMapping | null {
  const normalized = value.trim();
  if (!normalized) return null;
  if (protocol === "openai_chat_completions" || protocol === "openai_responses") {
    return { kind: "effort", value: normalized };
  }
  const budgetTokens = Number(normalized);
  if (Number.isInteger(budgetTokens) && /^\d+$/.test(normalized)) {
    return budgetTokens >= 1024
      ? { kind: "budget_tokens", value: budgetTokens }
      : null;
  }
  if (protocol === "anthropic_messages") {
    return { kind: "effort", value: normalized };
  }
  if (protocol === "gemini_generate_content") {
    return { kind: "native_level", value: normalized };
  }
  return null;
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
