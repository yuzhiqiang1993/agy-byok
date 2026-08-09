import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  AppConfig,
  ModelTokenLimits,
  Provider,
  UpstreamModel,
  VirtualModel,
} from "../../types/config";
import type { ConfigurableReasoningLevel, ReasoningLevel, ReasoningMapping } from "../../types/reasoning";
import {
  effectiveHostModelId,
  emptyParameters,
  nextHostModelId,
} from "../../utils/modelUtils";
import {
  catalogReasoningLevelsForModel,
  catalogReasoningMappingsForModel,
  customReasoningMapping,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import { tokenLimitsFromCatalog } from "./providerTokenPlan";

interface ProviderModelPlanInput {
  currentConfig: AppConfig;
  provider: Provider;
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
  providerUpstreams: UpstreamModel[];
  occupiedHostModelIds: Set<string>;
  protocolChanged: boolean;
  createId: () => string;
}

interface ExistingModelConfig {
  upstream: UpstreamModel | undefined;
  virtuals: VirtualModel[];
}

interface ExistingConfiguredModel {
  upstream: UpstreamModel;
  virtuals: VirtualModel[];
}

interface ReasoningPlan {
  reasoningEnabled: boolean;
  enabledLevels: ReasoningLevel[];
  reasoning: { levels: Partial<Record<ReasoningLevel, ReasoningMapping>> };
}

interface ProviderModelPlan {
  upstreams: UpstreamModel[];
  virtuals: VirtualModel[];
}

function existingModelConfig(
  input: ProviderModelPlanInput,
  modelId: string,
): ExistingModelConfig {
  const upstream = input.providerUpstreams.find((item) => item.upstream_model_id === modelId);
  const virtuals = upstream
    ? input.currentConfig.virtual_models.filter((item) => item.upstream_model_id === upstream.id)
    : [];
  return { upstream, virtuals };
}

function selectedReasoningLevels(
  input: ProviderModelPlanInput,
  modelId: string,
): Set<ConfigurableReasoningLevel> {
  return input.catalogReasoningEnabledModelIds.has(modelId)
    ? new Set(input.catalogReasoningLevelsByModel.get(modelId) ?? [])
    : new Set<ConfigurableReasoningLevel>();
}

function reasoningChanged(input: ProviderModelPlanInput, modelId: string): boolean {
  return input.changedCatalogReasoningModelIds.has(modelId) || input.protocolChanged;
}

function retainedReasoningLevels(
  input: ProviderModelPlanInput,
  modelId: string,
): Set<ReasoningLevel | null> {
  if (!input.catalogReasoningEnabledModelIds.has(modelId)) return new Set([null]);
  const levels: ReasoningLevel[] = [...selectedReasoningLevels(input, modelId)];
  if (input.catalogCustomReasoningByModel.has(modelId)) levels.push("auto");
  return new Set(levels);
}

function reserveRetainedHostModelIds(
  input: ProviderModelPlanInput,
  selectedModels: ProviderCatalogModel[],
): void {
  for (const model of selectedModels) {
    const existing = existingModelConfig(input, model.id);
    if (!existing.upstream) continue;
    if (!reasoningChanged(input, model.id)) {
      for (const virtualModel of existing.virtuals) {
        input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
      continue;
    }
    const retainedLevels = retainedReasoningLevels(input, model.id);
    for (const virtualModel of existing.virtuals) {
      if (retainedLevels.has(virtualModel.default_reasoning_level)) {
        input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
    }
  }
}

function buildReasoningPlan(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  existingUpstream: UpstreamModel | undefined,
): ReasoningPlan {
  const reasoningEnabled = input.catalogReasoningEnabledModelIds.has(model.id);
  const selectedLevels = selectedReasoningLevels(input, model.id);
  const availableMappings = catalogReasoningMappingsForModel(model, input.provider.protocol);
  const availableLevels = catalogReasoningLevelsForModel(model, input.provider.protocol, existingUpstream);
  const explicitCatalogMappings = model.reasoning?.mappings ?? {};
  const customReasoningValue = input.catalogCustomReasoningByModel.get(model.id);
  const customMapping = customReasoningValue
    ? customReasoningMapping(input.provider.protocol, customReasoningValue)
    : null;
  const enabledLevels: ReasoningLevel[] = reasoningEnabled
    ? [
        ...sortReasoningLevels([...selectedLevels].filter((level) => availableLevels.includes(level))),
        ...(customMapping ? ["auto" as const] : []),
      ]
    : [];
  const levels: Partial<Record<ReasoningLevel, ReasoningMapping>> = {};
  for (const level of enabledLevels) {
    const mapping = level === "auto"
      ? customMapping
      : (explicitCatalogMappings[level] !== undefined
          ? explicitCatalogMappings[level]
          : (input.protocolChanged
              ? undefined
              : existingUpstream?.capabilities.reasoning.levels[level]))
        ?? availableMappings[level];
    if (mapping) levels[level] = mapping;
  }
  return { reasoningEnabled, enabledLevels, reasoning: { levels } };
}

function updatedStableReasoningUpstream(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  existingUpstream: UpstreamModel,
): UpstreamModel {
  const capabilitiesChanged = input.changedCatalogCapabilityModelIds.has(model.id);
  return {
    ...existingUpstream,
    capabilities: {
      ...existingUpstream.capabilities,
      ...(capabilitiesChanged
        ? {
            vision: input.catalogVisionEnabledModelIds.has(model.id),
            tools: input.catalogToolsEnabledModelIds.has(model.id),
          }
        : {}),
    },
    token_limits: tokenLimitsFromCatalog(
      model,
      existingUpstream.token_limits,
      input.changedCatalogTokenLimitModelIds.has(model.id)
        ? input.catalogTokenLimitsByModel.get(model.id)
        : undefined,
    ),
  };
}

function buildUpstream(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  upstreamId: string,
  existingUpstream: UpstreamModel | undefined,
  reasoning: ReasoningPlan["reasoning"],
): UpstreamModel {
  const tokenLimits = tokenLimitsFromCatalog(
    model,
    existingUpstream?.token_limits,
    input.changedCatalogTokenLimitModelIds.has(model.id)
      ? input.catalogTokenLimitsByModel.get(model.id)
      : undefined,
  );
  const capabilities = {
    vision: input.catalogVisionEnabledModelIds.has(model.id),
    tools: input.catalogToolsEnabledModelIds.has(model.id),
    reasoning,
  };

  if (existingUpstream) {
    return {
      ...existingUpstream,
      capabilities,
      token_limits: tokenLimits,
    };
  }

  return {
    id: upstreamId,
    provider_id: input.provider.id,
    upstream_model_id: model.id,
    display_name: model.displayName,
    capabilities,
    token_limits: tokenLimits,
    compression_policy: null,
    tokenizer: null,
    parameter_overrides: emptyParameters(),
    enabled: true,
  };
}

function newVirtualModel(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  upstreamId: string,
  id: string,
  defaultReasoningLevel: ReasoningLevel | null,
): VirtualModel {
  return {
    id: `custom-${id}`,
    host_model_id: nextHostModelId(input.occupiedHostModelIds),
    upstream_model_id: upstreamId,
    display_name: model.displayName,
    default_reasoning_level: defaultReasoningLevel,
    parameter_overrides: emptyParameters(),
    fallback_virtual_model_id: null,
    enabled: true,
  };
}

function stableReasoningModelPlan(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  upstreamId: string,
  modelId: string,
  existing: ExistingConfiguredModel,
): ProviderModelPlan {
  const virtuals = [...existing.virtuals];
  for (const virtualModel of virtuals) {
    input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
  }
  if (virtuals.length === 0) {
    virtuals.push(newVirtualModel(input, model, upstreamId, modelId, null));
  }
  return {
    upstreams: [updatedStableReasoningUpstream(input, model, existing.upstream)],
    virtuals,
  };
}

function changedReasoningModelPlan(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
  upstreamId: string,
  existing: ExistingModelConfig,
): ProviderModelPlan {
  const reasoningPlan = buildReasoningPlan(input, model, existing.upstream);
  const retainedLevels = new Set<ReasoningLevel | null>(
    reasoningPlan.reasoningEnabled ? reasoningPlan.enabledLevels : [null],
  );
  for (const virtualModel of existing.virtuals) {
    if (retainedLevels.has(virtualModel.default_reasoning_level)) {
      input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
    }
  }
  const desiredLevels: Array<ReasoningLevel | null> = reasoningPlan.reasoningEnabled
    ? reasoningPlan.enabledLevels
    : [null];
  const virtuals: VirtualModel[] = [];
  for (const defaultReasoningLevel of desiredLevels) {
    const matchingVirtuals = existing.virtuals.filter(
      (virtualModel) => virtualModel.default_reasoning_level === defaultReasoningLevel,
    );
    if (matchingVirtuals.length > 0) {
      for (const virtualModel of matchingVirtuals) {
        input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
        virtuals.push(virtualModel);
      }
      continue;
    }
    virtuals.push(newVirtualModel(
      input,
      model,
      upstreamId,
      input.createId(),
      defaultReasoningLevel,
    ));
  }
  return {
    upstreams: [buildUpstream(input, model, upstreamId, existing.upstream, reasoningPlan.reasoning)],
    virtuals,
  };
}

function selectedModelPlan(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
): ProviderModelPlan {
  const existing = existingModelConfig(input, model.id);
  const modelId = input.createId();
  const upstreamId = existing.upstream?.id ?? `upstream-${modelId}`;
  if (existing.upstream && !reasoningChanged(input, model.id)) {
    return stableReasoningModelPlan(
      input,
      model,
      upstreamId,
      modelId,
      { ...existing, upstream: existing.upstream },
    );
  }
  return changedReasoningModelPlan(input, model, upstreamId, existing);
}

export function buildProviderModelPlan(input: ProviderModelPlanInput): ProviderModelPlan {
  const selectedModels = input.catalogModels.filter((model) => input.selectedCatalogModelIds.has(model.id));
  reserveRetainedHostModelIds(input, selectedModels);
  const plan: ProviderModelPlan = { upstreams: [], virtuals: [] };
  for (const model of selectedModels) {
    const modelPlan = selectedModelPlan(input, model);
    plan.upstreams.push(...modelPlan.upstreams);
    plan.virtuals.push(...modelPlan.virtuals);
  }
  return plan;
}
