import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  AppConfig,
  ModelTokenLimits,
  Provider,
  UpstreamModel,
  VirtualModel,
} from "../../types/config";
import type { ProviderChangeSummary, ProviderSavePlan } from "../../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningLevel, ReasoningMapping } from "../../types/reasoning";
import {
  effectiveHostModelId,
  emptyParameters,
  nextHostModelId,
  stripConfiguredModelSuffix,
} from "../../utils/modelUtils";
import {
  catalogContextWindow,
  DEFAULT_CONTEXT_WINDOW,
  DEFAULT_TOKEN_LIMIT,
} from "./tokenLimits";
import {
  catalogReasoningLevelsForModel,
  catalogReasoningMappingsForModel,
  customReasoningMapping,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";

export interface ProviderSavePlanInput {
  currentConfig: AppConfig;
  provider: Provider;
  editingProviderId: string | null;
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogVisionEnabledModelIds: ReadonlySet<string>;
  catalogToolsEnabledModelIds: ReadonlySet<string>;
  catalogReasoningEnabledModelIds: ReadonlySet<string>;
  catalogTokenLimitsByModel: ReadonlyMap<string, ModelTokenLimits>;
  changedCatalogTokenLimitModelIds: ReadonlySet<string>;
  changedCatalogCapabilityModelIds: ReadonlySet<string>;
  changedCatalogReasoningModelIds: ReadonlySet<string>;
  legacyCatalogModelIds: ReadonlySet<string>;
  createId: () => string;
}

function tokenLimitsFromCatalog(
  model: ProviderCatalogModel,
  existing: ModelTokenLimits | undefined,
  selected: ModelTokenLimits | undefined,
): ModelTokenLimits {
  const configured = selected ?? existing;
  return {
    // 供应商目录值优先；目录缺失时沿用历史值，否则使用经验默认值。
    context_window: catalogContextWindow(model) ?? configured?.context_window ?? DEFAULT_CONTEXT_WINDOW,
    input_token_limit: model.inputTokenLimit ?? configured?.input_token_limit ?? DEFAULT_TOKEN_LIMIT,
    output_token_limit: model.outputTokenLimit ?? configured?.output_token_limit ?? DEFAULT_TOKEN_LIMIT,
  };
}

export function summarizeProviderChanges(
  currentConfig: AppConfig,
  providerId: string,
  nextConfig: AppConfig,
  legacyCatalogModelIds: ReadonlySet<string>,
  selectedCatalogModelIds: ReadonlySet<string>,
): ProviderChangeSummary {
  const currentUpstreams = currentConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const nextUpstreams = nextConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const currentUpstreamIds = new Set(currentUpstreams.map((item) => item.id));
  const nextUpstreamIds = new Set(nextUpstreams.map((item) => item.id));
  const currentVirtuals = currentConfig.virtual_models.filter(
    (item) => currentUpstreamIds.has(item.upstream_model_id),
  );
  const nextVirtuals = nextConfig.virtual_models.filter(
    (item) => nextUpstreamIds.has(item.upstream_model_id),
  );
  const currentVirtualIds = new Set(currentVirtuals.map((item) => item.id));
  const nextVirtualIds = new Set(nextVirtuals.map((item) => item.id));
  const fallbackBlockers = nextConfig.virtual_models.flatMap((model) =>
    model.fallback_virtual_model_id
      && !nextVirtualIds.has(model.fallback_virtual_model_id)
      && !nextConfig.virtual_models.some((candidate) => candidate.id === model.fallback_virtual_model_id)
      ? [{
          source: model.display_name,
          fallback: nextConfig.virtual_models.find((candidate) => candidate.id === model.fallback_virtual_model_id)?.display_name
            ?? model.fallback_virtual_model_id,
        }]
      : [],
  );

  return {
    addedUpstreamIds: nextUpstreams
      .filter((item) => !currentUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    removedUpstreamIds: currentUpstreams
      .filter((item) => !nextUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    addedVirtualModels: nextVirtuals.filter((item) => !currentVirtualIds.has(item.id)),
    removedVirtualModels: currentVirtuals.filter((item) => !nextVirtualIds.has(item.id)),
    retainedVirtualCount: nextVirtuals.filter((item) => currentVirtualIds.has(item.id)).length,
    legacyModelIds: [...legacyCatalogModelIds].filter((id) => selectedCatalogModelIds.has(id)),
    fallbackBlockers,
  };
}

export function buildProviderSavePlan(input: ProviderSavePlanInput): ProviderSavePlan {
  const {
    currentConfig,
    provider,
    editingProviderId,
    catalogModels,
    selectedCatalogModelIds,
    catalogReasoningLevelsByModel,
    catalogCustomReasoningByModel,
    catalogVisionEnabledModelIds,
    catalogToolsEnabledModelIds,
    catalogReasoningEnabledModelIds,
    catalogTokenLimitsByModel,
    changedCatalogTokenLimitModelIds,
    changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds,
    legacyCatalogModelIds,
    createId,
  } = input;
  const previousProvider = editingProviderId
    ? currentConfig.providers.find((item) => item.id === editingProviderId)
    : undefined;
  const providerUpstreams = currentConfig.upstream_models.filter((item) => item.provider_id === provider.id);
  const providerUpstreamIds = new Set(providerUpstreams.map((item) => item.id));
  const remainingUpstreams = currentConfig.upstream_models.filter((item) => item.provider_id !== provider.id);
  const remainingVirtuals = currentConfig.virtual_models.filter(
    (item) => !providerUpstreamIds.has(item.upstream_model_id),
  );
  const occupiedHostModelIds = new Set(remainingVirtuals.map(effectiveHostModelId));
  const selectedModels = catalogModels.filter((model) => selectedCatalogModelIds.has(model.id));
  const protocolChanged = previousProvider !== undefined && previousProvider.protocol !== provider.protocol;
  const nextUpstreams: UpstreamModel[] = [];
  const nextVirtuals: VirtualModel[] = [];
  const reasoningLevelsForModel = (modelId: string): Set<ConfigurableReasoningLevel> =>
    catalogReasoningEnabledModelIds.has(modelId)
      ? new Set(catalogReasoningLevelsByModel.get(modelId) ?? [])
      : new Set<ConfigurableReasoningLevel>();

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find((item) => item.upstream_model_id === model.id);
    if (!existingUpstream) continue;

    const existingVirtuals = currentConfig.virtual_models.filter(
      (item) => item.upstream_model_id === existingUpstream.id,
    );
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    if (!reasoningChanged) {
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
      continue;
    }

    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id);
    const selectedReasoningLevels = reasoningLevelsForModel(model.id);
    const customReasoningSelected = catalogCustomReasoningByModel.has(model.id);
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      reasoningEnabled
        ? [...selectedReasoningLevels, ...(customReasoningSelected ? ["auto" as const] : [])]
        : [null],
    );
    for (const virtualModel of existingVirtuals) {
      if (retainedReasoningLevels.has(virtualModel.default_reasoning_level)) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
    }
  }

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find((item) => item.upstream_model_id === model.id);
    const existingVirtuals = existingUpstream
      ? currentConfig.virtual_models.filter((item) => item.upstream_model_id === existingUpstream.id)
      : [];
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    const capabilitiesChanged = changedCatalogCapabilityModelIds.has(model.id);
    const vision = catalogVisionEnabledModelIds.has(model.id);
    const tools = catalogToolsEnabledModelIds.has(model.id);
    const id = createId();
    const upstreamId = existingUpstream?.id ?? `upstream-${id}`;

    if (existingUpstream && !reasoningChanged) {
      nextUpstreams.push({
        ...existingUpstream,
        capabilities: {
          ...existingUpstream.capabilities,
          ...(capabilitiesChanged ? { vision, tools } : {}),
        },
        token_limits: tokenLimitsFromCatalog(
          model,
          existingUpstream.token_limits,
          changedCatalogTokenLimitModelIds.has(model.id)
            ? catalogTokenLimitsByModel.get(model.id)
            : undefined,
        ),
      });
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
        nextVirtuals.push(virtualModel);
      }
      if (existingVirtuals.length > 0) continue;

      nextVirtuals.push({
        id: `custom-${id}`,
        host_model_id: nextHostModelId(occupiedHostModelIds),
        upstream_model_id: upstreamId,
        display_name: model.displayName,
        default_reasoning_level: null,
        parameter_overrides: emptyParameters(),
        fallback_virtual_model_id: null,
        enabled: true,
      });
      continue;
    }

    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id);
    const selectedReasoningLevels = reasoningLevelsForModel(model.id);
    const availableMappings = catalogReasoningMappingsForModel(model, provider.protocol);
    const availableReasoningLevels = catalogReasoningLevelsForModel(model, provider.protocol, existingUpstream);
    const explicitCatalogMappings = model.reasoning?.mappings ?? {};
    const customReasoningValue = catalogCustomReasoningByModel.get(model.id);
    const customMapping = customReasoningValue
      ? customReasoningMapping(provider.protocol, customReasoningValue)
      : null;
    const enabledLevels: ReasoningLevel[] = reasoningEnabled
      ? [
          ...sortReasoningLevels(
            [...selectedReasoningLevels].filter((level) => availableReasoningLevels.includes(level)),
          ),
          ...(customMapping ? ["auto" as const] : []),
        ]
      : [];
    const levels: Partial<Record<ReasoningLevel, ReasoningMapping>> = {};
    for (const level of enabledLevels) {
      const mapping = level === "auto"
        ? customMapping
        : (explicitCatalogMappings[level] !== undefined
            ? explicitCatalogMappings[level]
            : (protocolChanged
                ? undefined
                : existingUpstream?.capabilities.reasoning.levels[level]))
          ?? availableMappings[level];
      if (mapping) levels[level] = mapping;
    }
    const reasoning = { levels };
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      reasoningEnabled ? [...enabledLevels] : [null],
    );
    for (const virtualModel of existingVirtuals) {
      if (retainedReasoningLevels.has(virtualModel.default_reasoning_level)) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
    }
    nextUpstreams.push(existingUpstream
      ? {
          ...existingUpstream,
          capabilities: { ...existingUpstream.capabilities, vision, tools, reasoning },
          token_limits: tokenLimitsFromCatalog(
            model,
            existingUpstream.token_limits,
            changedCatalogTokenLimitModelIds.has(model.id)
              ? catalogTokenLimitsByModel.get(model.id)
              : undefined,
          ),
        }
      : {
          id: upstreamId,
          provider_id: provider.id,
          upstream_model_id: model.id,
          display_name: model.displayName,
          capabilities: { vision, tools, reasoning },
          token_limits: tokenLimitsFromCatalog(
            model,
            undefined,
            changedCatalogTokenLimitModelIds.has(model.id)
              ? catalogTokenLimitsByModel.get(model.id)
              : undefined,
          ),
          parameter_overrides: emptyParameters(),
          enabled: true,
        });

    const desiredReasoningLevels: Array<ReasoningLevel | null> = reasoningEnabled ? enabledLevels : [null];
    for (const defaultReasoningLevel of desiredReasoningLevels) {
      const matchingVirtuals = existingVirtuals.filter(
        (virtualModel) => virtualModel.default_reasoning_level === defaultReasoningLevel,
      );
      if (matchingVirtuals.length > 0) {
        for (const virtualModel of matchingVirtuals) {
          occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
          nextVirtuals.push(virtualModel);
        }
        continue;
      }

      const virtualId = createId();
      nextVirtuals.push({
        id: `custom-${virtualId}`,
        host_model_id: nextHostModelId(occupiedHostModelIds),
        upstream_model_id: upstreamId,
        display_name: model.displayName,
        default_reasoning_level: defaultReasoningLevel,
        parameter_overrides: emptyParameters(),
        fallback_virtual_model_id: null,
        enabled: true,
      });
    }
  }

  const providers = editingProviderId
    ? currentConfig.providers.map((item) => item.id === provider.id ? provider : item)
    : [...currentConfig.providers, provider];
  const providerRenamed = previousProvider !== undefined && previousProvider.name !== provider.name;
  const providerVirtuals = providerRenamed
    ? nextVirtuals.map((virtualModel) => ({
        ...virtualModel,
        display_name: stripConfiguredModelSuffix(virtualModel.display_name, previousProvider.name),
      }))
    : nextVirtuals;
  const nextConfig: AppConfig = {
    proxy_port: currentConfig.proxy_port,
    providers,
    upstream_models: [...remainingUpstreams, ...nextUpstreams],
    virtual_models: [...remainingVirtuals, ...providerVirtuals],
    official_model_settings: currentConfig.official_model_settings,
  };
  const summary = summarizeProviderChanges(
    currentConfig,
    provider.id,
    nextConfig,
    legacyCatalogModelIds,
    selectedCatalogModelIds,
  );

  return {
    provider,
    nextConfig,
    summary,
    wasEditing: editingProviderId !== null,
  };
}
