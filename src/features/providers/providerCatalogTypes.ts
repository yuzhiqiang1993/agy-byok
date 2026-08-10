import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  ModelTokenLimits,
  Provider,
  ProviderProtocol,
} from "../../types/config";
import type { ConfigurableReasoningLevel, ThinkingBudgetConfig } from "../../types/reasoning";

export interface ProviderCatalogState {
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
  unavailableCatalogModelIds: ReadonlySet<string>;
}

export interface ProviderCatalogContext {
  getEditingProviderId: () => string | null;
  selectedProtocol: () => ProviderProtocol;
  providerFromForm: () => Provider;
  setProviderEditorDirty: (dirty: boolean) => void;
  withProviderEditorBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
  invalidatePendingProviderSave: () => void;
  refreshProviderEditorControls: () => void;
}

export interface CatalogControlState {
  catalogTokenLimitsByModel: Map<string, ModelTokenLimits>;
  changedCatalogTokenLimitModelIds: Set<string>;
}

export interface CatalogModelListState extends CatalogControlState {
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: Set<string>;
  catalogReasoningLevelsByModel: Map<string, Set<ConfigurableReasoningLevel>>;
  catalogThinkingBudgetsByModel: Map<string, ThinkingBudgetConfig>;
  catalogVisionEnabledModelIds: Set<string>;
  catalogVideoEnabledModelIds: Set<string>;
  catalogSupportedMimeTypesByModel: Map<string, Set<string>>;
  catalogToolsEnabledModelIds: Set<string>;
  catalogReasoningEnabledModelIds: Set<string>;
  changedCatalogCapabilityModelIds: Set<string>;
  changedCatalogReasoningModelIds: Set<string>;
  unavailableCatalogModelIds: Set<string>;
  expandedCatalogModelIds: Set<string>;
}
