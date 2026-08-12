import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { UpstreamModel } from "../../types/config";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import { catalogReasoningLevelsForModel } from "../../utils/reasoningUtils";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";

export interface CatalogModelRowState {
  model: ProviderCatalogModel;
  selected: boolean;
  expanded: boolean;
  existingUpstream: UpstreamModel | undefined;
  selectedReasoningLevels: Set<ConfigurableReasoningLevel> | undefined;
  availableReasoningLevels: ConfigurableReasoningLevel[];
  reasoningEnabled: boolean;
}

export function resolveCatalogModelRowState(
  model: ProviderCatalogModel,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): CatalogModelRowState {
  const editingProviderId = context.getEditingProviderId();
  const existingUpstream = editingProviderId
    ? store.config.upstream_models.find(
        (item) => item.provider_id === editingProviderId && item.upstream_model_id === model.id,
      )
    : undefined;
  const selectedReasoningLevels = state.catalogReasoningLevelsByModel.get(model.id);
  const thinkingBudgets = state.catalogThinkingBudgetsByModel.get(model.id);
  const hasReasoningConfiguration = (selectedReasoningLevels?.size ?? 0) > 0
    || thinkingBudgets?.thinkingBudget != null
    || thinkingBudgets?.minThinkingBudget != null;

  return {
    model,
    selected: state.selectedCatalogModelIds.has(model.id),
    expanded: state.expandedCatalogModelIds.has(model.id),
    existingUpstream,
    selectedReasoningLevels,
    availableReasoningLevels: catalogReasoningLevelsForModel(
      model,
      context.selectedProtocol(),
      existingUpstream,
      state.catalogTokenLimitsByModel.get(model.id)?.output_token_limit ?? null,
    ),
    reasoningEnabled: state.catalogReasoningEnabledModelIds.has(model.id)
      && hasReasoningConfiguration,
  };
}
