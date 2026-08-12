import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  AppConfig,
  ModelTokenLimits,
  Provider,
  UpstreamModel,
  VirtualModel,
} from "../../types/config";
import type {
  ConfigurableReasoningLevel,
  ReasoningLevel,
  ReasoningMapping,
  ThinkingBudgetConfig,
} from "../../types/reasoning";
import {
  effectiveHostModelId,
  emptyParameters,
  nextHostModelId,
} from "../../utils/modelUtils";
import {
  catalogReasoningLevelsForModel,
  customReasoningMapping,
  resolveReasoningMappingForModel,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import {
  catalogSupportedMimeTypes,
  normalizeMediaMimeTypes,
  supportsVideoInput,
} from "./modelMediaCapabilities";
import { tokenLimitsFromCatalog } from "./providerTokenPlan";

interface ProviderModelPlanInput {
  currentConfig: AppConfig;
  provider: Provider;
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogThinkingBudgetsByModel: ReadonlyMap<string, ThinkingBudgetConfig>;
  catalogVisionEnabledModelIds: ReadonlySet<string>;
  catalogVideoEnabledModelIds: ReadonlySet<string>;
  catalogSupportedMimeTypesByModel: ReadonlyMap<string, ReadonlySet<string>>;
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
  reasoning: {
    supported: boolean;
    thinking_budget: number | null;
    min_thinking_budget: number | null;
    levels: Partial<Record<ReasoningLevel, ReasoningMapping>>;
  };
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

function selectedSupportedMimeTypes(
  input: ProviderModelPlanInput,
  model: ProviderCatalogModel,
): string[] {
  const selectedMimeTypes = input.catalogSupportedMimeTypesByModel.get(model.id)
    ?? catalogSupportedMimeTypes(model);
  return normalizeMediaMimeTypes(selectedMimeTypes, {
    supportsImages: input.catalogVisionEnabledModelIds.has(model.id),
    supportsVideo: input.catalogVideoEnabledModelIds.has(model.id),
    // 仅原生 Gemini 请求适配器能够安全转发视频内容。
    videoAvailable: supportsVideoInput(input.provider.protocol),
  });
}

function defaultCompressionPolicy(
  model: ProviderCatalogModel,
  tokenLimits: ModelTokenLimits,
): UpstreamModel["compression_policy"] {
  const policy = model.defaultCompressionPolicy;
  if (!policy?.enabled) return null;
  const trustedCapacity = [
    tokenLimits.context_window_source === "catalog" || tokenLimits.context_window_source === "configured"
      ? tokenLimits.context_window
      : null,
    tokenLimits.input_token_limit_source === "catalog" || tokenLimits.input_token_limit_source === "configured"
      ? tokenLimits.input_token_limit
      : null,
  ].filter((value): value is number => value != null && value > 0);
  if (trustedCapacity.length === 0 || policy.max_token_limit > Math.min(...trustedCapacity)) {
    return null;
  }
  const trustedOutputLimit = tokenLimits.output_token_limit_source === "catalog"
    || tokenLimits.output_token_limit_source === "configured"
    ? tokenLimits.output_token_limit
    : null;
  if (trustedOutputLimit != null && policy.max_output_tokens > trustedOutputLimit) return null;
  return { ...policy, retry_config: { ...policy.retry_config } };
}

function retainedReasoningLevels(
  input: ProviderModelPlanInput,
  modelId: string,
): Set<ReasoningLevel | null> {
  if (!input.catalogReasoningEnabledModelIds.has(modelId)) return new Set([null]);
  const levels: ReasoningLevel[] = [...selectedReasoningLevels(input, modelId)];
  if (input.catalogCustomReasoningByModel.has(modelId)) levels.push("auto");
  return new Set<ReasoningLevel | null>(levels.length > 0 ? levels : [null]);
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
  const outputTokenLimit = input.catalogTokenLimitsByModel.get(model.id)?.output_token_limit
    ?? model.outputTokenLimit
    ?? null;
  const availableLevels = catalogReasoningLevelsForModel(
    model,
    input.provider.protocol,
    existingUpstream,
    outputTokenLimit,
  );
  const customReasoningValue = input.catalogCustomReasoningByModel.get(model.id);
  const customMapping = customReasoningValue
    ? customReasoningMapping(input.provider.protocol, customReasoningValue, outputTokenLimit)
    : null;
  const enabledLevels: Array<ConfigurableReasoningLevel | "auto"> = reasoningEnabled
    ? [
        ...sortReasoningLevels([...selectedLevels].filter((level) => availableLevels.includes(level))),
        ...(customMapping ? ["auto" as const] : []),
      ]
    : [];
  const levels: Partial<Record<ReasoningLevel, ReasoningMapping>> = {};
  for (const level of enabledLevels) {
    const mapping = level === "auto"
      ? customMapping
      : resolveReasoningMappingForModel(
          model,
          input.provider.protocol,
          level,
          input.protocolChanged ? undefined : existingUpstream,
          outputTokenLimit,
        ).mapping;
    if (mapping) levels[level] = mapping;
  }
  const budgets = input.catalogThinkingBudgetsByModel.get(model.id);
  return {
    reasoningEnabled,
    enabledLevels,
    reasoning: {
      supported: reasoningEnabled,
      thinking_budget: reasoningEnabled ? budgets?.thinkingBudget ?? null : null,
      min_thinking_budget: reasoningEnabled ? budgets?.minThinkingBudget ?? null : null,
      levels,
    },
  };
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
      supported_mime_types: selectedSupportedMimeTypes(input, model),
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
    supported_mime_types: selectedSupportedMimeTypes(input, model),
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
    compression_policy: defaultCompressionPolicy(model, tokenLimits),
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
    reasoningPlan.reasoningEnabled && reasoningPlan.enabledLevels.length > 0
      ? reasoningPlan.enabledLevels
      : [null],
  );
  for (const virtualModel of existing.virtuals) {
    if (retainedLevels.has(virtualModel.default_reasoning_level)) {
      input.occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
    }
  }
  const desiredLevels: Array<ReasoningLevel | null> = reasoningPlan.reasoningEnabled
    ? reasoningPlan.enabledLevels.length > 0 ? reasoningPlan.enabledLevels : [null]
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
