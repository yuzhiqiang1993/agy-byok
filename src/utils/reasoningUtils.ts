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
  adaptive: 6,
  auto: 7,
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
    adaptive: t("models.reasoningAdaptive"),
    auto: t("models.reasoningCustom"),
  }[level];
}

function configurableReasoningLevels(protocol: ProviderProtocol): ConfigurableReasoningLevel[] {
  if (protocol === "gemini_generate_content") return ["low", "medium", "high"];
  if (protocol === "anthropic_messages") {
    return ["low", "medium", "high", "x_high", "max", "adaptive"];
  }
  return ["low", "medium", "high", "x_high", "max"];
}

export function catalogReasoningIsAuthoritative(model: ProviderCatalogModel): boolean {
  return (model.reasoning?.levels ?? []).some((level) => level !== "off" && level !== "auto")
    || Object.keys(model.reasoning?.mappings ?? {}).some((level) => level !== "off" && level !== "auto");
}

export function catalogReasoningLevelsForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  existingUpstream?: UpstreamModel,
  outputTokenLimit?: number | null,
): ConfigurableReasoningLevel[] {
  const configurable = configurableReasoningLevels(protocol);
  const effectiveOutputTokenLimit = outputTokenLimit
    ?? model.outputTokenLimit
    ?? trustedConfiguredOutputLimit(existingUpstream);
  const existing = existingUpstream
    ? (Object.keys(existingUpstream.capabilities.reasoning.levels) as ReasoningLevel[]).filter(
        (level): level is ConfigurableReasoningLevel =>
          configurable.includes(level as ConfigurableReasoningLevel)
          && reasoningMappingSupported(
            protocol,
            existingUpstream.capabilities.reasoning.levels[level],
            effectiveOutputTokenLimit,
          ),
      )
    : [];

  if (model.reasoning?.supported === false) return sortReasoningLevels([...new Set(existing)]);

  const mappings = catalogReasoningMappingsForModel(model, protocol, effectiveOutputTokenLimit);
  const explicit = (model.reasoning?.levels ?? []).filter(
    (level): level is ConfigurableReasoningLevel =>
      configurable.includes(level as ConfigurableReasoningLevel)
      && (mappings[level] !== undefined || existing.includes(level as ConfigurableReasoningLevel)),
  );
  if (explicit.length > 0) {
    return sortReasoningLevels([...new Set([...existing, ...explicit])]);
  }

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

export type ReasoningMappingSource = "catalog" | "configured" | "protocol_suggestion";

export function reasoningMappingSource(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  level: ConfigurableReasoningLevel,
  existingUpstream?: UpstreamModel,
  outputTokenLimit?: number | null,
): ReasoningMappingSource {
  return resolveReasoningMappingForModel(
    model,
    protocol,
    level,
    existingUpstream,
    outputTokenLimit,
  ).source;
}

export type ReasoningConfigurationSource =
  | "catalog"
  | "catalog_adaptive"
  | "catalog_capability"
  | "configured"
  | "protocol_suggestion";

export function reasoningConfigurationSource(
  model: ProviderCatalogModel,
  existingUpstream?: UpstreamModel,
): ReasoningConfigurationSource {
  if (Object.values(model.reasoning?.mappings ?? {}).some((mapping) => mapping.kind === "adaptive")) return "catalog_adaptive";
  if (catalogReasoningIsAuthoritative(model)) return "catalog";
  if (model.reasoning !== undefined) return "catalog_capability";
  const configured = existingUpstream?.capabilities.reasoning;
  if (configured && (
    configured.supported !== null
    || configured.thinking_budget !== null
    || configured.min_thinking_budget !== null
    || Object.keys(configured.levels).length > 0
  )) {
    return "configured";
  }
  return "protocol_suggestion";
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
    if (level === "adaptive") return { kind: "adaptive" };
    const budgetTokens: Record<Exclude<ConfigurableReasoningLevel, "adaptive">, number> = {
      low: 1_024,
      medium: 4_096,
      high: 8_192,
      x_high: 16_384,
      max: 32_768,
    };
    return { kind: "budget_tokens", value: budgetTokens[level] };
  }
  if (protocol === "gemini_generate_content") {
    return { kind: "native_level", value: level === "x_high" ? "xhigh" : level };
  }
  return { kind: "effort", value: level === "x_high" ? "xhigh" : level };
}

function trustedConfiguredOutputLimit(upstream?: UpstreamModel): number | null {
  if (!upstream) return null;
  const { output_token_limit: limit, output_token_limit_source: source } = upstream.token_limits;
  return limit != null && (source === "catalog" || source === "configured") ? limit : null;
}

function reasoningMappingSupported(
  protocol: ProviderProtocol,
  mapping: ReasoningMapping | undefined,
  outputTokenLimit: number | null,
): boolean {
  if (!mapping) return false;
  if (protocol === "anthropic_messages" && mapping.kind === "budget_tokens") {
    return mapping.value >= 1_024
      && (outputTokenLimit == null || mapping.value < outputTokenLimit);
  }
  if (protocol === "openai_chat_completions" || protocol === "openai_responses") {
    return mapping.kind === "effort" || mapping.kind === "disabled";
  }
  if (protocol === "gemini_generate_content") {
    return mapping.kind === "native_level"
      || mapping.kind === "budget_tokens"
      || mapping.kind === "disabled";
  }
  return mapping.kind === "effort"
    || mapping.kind === "adaptive"
    || mapping.kind === "disabled";
}

export function resolveReasoningMappingForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  level: ConfigurableReasoningLevel,
  existingUpstream?: UpstreamModel,
  outputTokenLimit?: number | null,
): { mapping: ReasoningMapping | null; source: ReasoningMappingSource } {
  const effectiveOutputTokenLimit = outputTokenLimit
    ?? model.outputTokenLimit
    ?? trustedConfiguredOutputLimit(existingUpstream);
  const catalogMapping = model.reasoning?.mappings?.[level];
  if (reasoningMappingSupported(protocol, catalogMapping, effectiveOutputTokenLimit)) {
    return { mapping: catalogMapping ?? null, source: "catalog" };
  }
  const configuredMapping = existingUpstream?.capabilities.reasoning.levels[level];
  if (reasoningMappingSupported(protocol, configuredMapping, effectiveOutputTokenLimit)) {
    return { mapping: configuredMapping ?? null, source: "configured" };
  }
  return {
    mapping: catalogReasoningMappingsForModel(model, protocol, effectiveOutputTokenLimit)[level]
      ?? null,
    source: "protocol_suggestion",
  };
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
  outputTokenLimit: number | null = model.outputTokenLimit ?? null,
): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  const mappings: Partial<Record<ReasoningLevel, ReasoningMapping>> = {};
  for (const [level, mapping] of Object.entries(model.reasoning?.mappings ?? {}) as Array<[
    ReasoningLevel,
    ReasoningMapping,
  ]>) {
    if (reasoningMappingSupported(protocol, mapping, outputTokenLimit)) mappings[level] = mapping;
  }
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
    if (reasoningMappingSupported(protocol, mapping, outputTokenLimit)) mappings[level] = mapping;
  }
  return mappings;
}

export function customReasoningMapping(
  protocol: ProviderProtocol,
  value: string,
  outputTokenLimit: number | null = null,
): ReasoningMapping | null {
  const normalized = value.trim();
  if (!normalized) return null;
  if (protocol === "openai_chat_completions" || protocol === "openai_responses") {
    return { kind: "effort", value: normalized };
  }
  const budgetTokens = Number(normalized);
  if (Number.isInteger(budgetTokens) && /^\d+$/.test(normalized)) {
    if (budgetTokens < 1_024) return null;
    const mapping: ReasoningMapping = { kind: "budget_tokens", value: budgetTokens };
    return reasoningMappingSupported(protocol, mapping, outputTokenLimit) ? mapping : null;
  }
  if (protocol === "anthropic_messages") {
    if (normalized.toLowerCase() === "adaptive") return { kind: "adaptive" };
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
