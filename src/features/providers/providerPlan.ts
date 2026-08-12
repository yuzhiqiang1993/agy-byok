import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  AppConfig,
  ModelTokenLimits,
  Provider,
} from "../../types/config";
import type { ProviderSavePlan } from "../../types/proxy";
import type { ConfigurableReasoningLevel, ThinkingBudgetConfig } from "../../types/reasoning";
import { effectiveHostModelId, stripConfiguredModelSuffix } from "../../utils/modelUtils";
import { summarizeProviderChanges } from "./providerChangeSummary";
import { buildProviderModelPlan } from "./providerModelPlan";

interface ProviderSavePlanInput {
  currentConfig: AppConfig;
  provider: Provider;
  editingProviderId: string | null;
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogThinkingBudgetsByModel: ReadonlyMap<string, ThinkingBudgetConfig>;
  catalogImageInputModelIds: ReadonlySet<string>;
  catalogAudioInputModelIds: ReadonlySet<string>;
  catalogVideoInputModelIds: ReadonlySet<string>;
  catalogDocumentInputModelIds: ReadonlySet<string>;
  catalogInputMimeTypesByModel: ReadonlyMap<string, ReadonlySet<string>>;
  catalogToolsEnabledModelIds: ReadonlySet<string>;
  catalogReasoningEnabledModelIds: ReadonlySet<string>;
  catalogTokenLimitsByModel: ReadonlyMap<string, ModelTokenLimits>;
  changedCatalogTokenLimitModelIds: ReadonlySet<string>;
  changedCatalogCapabilityModelIds: ReadonlySet<string>;
  changedCatalogReasoningModelIds: ReadonlySet<string>;
  unavailableCatalogModelIds: ReadonlySet<string>;
  createId: () => string;
}

export function buildProviderSavePlan(input: ProviderSavePlanInput): ProviderSavePlan {
  const {
    currentConfig,
    provider,
    editingProviderId,
    selectedCatalogModelIds,
    unavailableCatalogModelIds,
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
  const protocolChanged = previousProvider !== undefined && previousProvider.protocol !== provider.protocol;
  const modelPlan = buildProviderModelPlan({
    ...input,
    providerUpstreams,
    occupiedHostModelIds,
    protocolChanged,
  });

  const providers = editingProviderId
    ? currentConfig.providers.map((item) => item.id === provider.id ? provider : item)
    : [...currentConfig.providers, provider];
  const providerRenamed = previousProvider !== undefined && previousProvider.name !== provider.name;
  const providerVirtuals = providerRenamed
    ? modelPlan.virtuals.map((virtualModel) => ({
        ...virtualModel,
        display_name: stripConfiguredModelSuffix(virtualModel.display_name, previousProvider.name),
      }))
    : modelPlan.virtuals;
  const nextConfig: AppConfig = {
    ...currentConfig,
    providers,
    upstream_models: [...remainingUpstreams, ...modelPlan.upstreams],
    virtual_models: [...remainingVirtuals, ...providerVirtuals],
  };
  const summary = summarizeProviderChanges(
    currentConfig,
    provider.id,
    nextConfig,
    unavailableCatalogModelIds,
    selectedCatalogModelIds,
  );

  return {
    provider,
    nextConfig,
    summary,
    wasEditing: editingProviderId !== null,
  };
}
