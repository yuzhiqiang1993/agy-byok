import type { ProviderChangeSummary, ProviderSavePlan } from "../../types/proxy";
import type { Provider } from "../../types/config";
import { store } from "../../store/appStore";
import { persistProviderConfig } from "../../controllers/providerController";
import {
  connectionTestResults,
  providerTestSessions,
  setActiveProviderTabId,
} from "./providerState";
import { buildProviderSavePlan } from "./providerPlan";
import type { ProviderCatalogState } from "./providerCatalog";
import { element } from "../../utils/domUtils";
import { customReasoningMapping } from "../../utils/reasoningUtils";
import { getLanguage, t } from "../../i18n";
import type { ProviderCatalogModel } from "../../types/catalog";

let pendingProviderSavePlan: ProviderSavePlan | null = null;

export function getPendingProviderSavePlan(): ProviderSavePlan | null {
  return pendingProviderSavePlan;
}

export function invalidatePendingProviderSave(): void {
  pendingProviderSavePlan = null;
  const providerChangeSummary = element<HTMLElement>("#provider-change-summary");
  providerChangeSummary.hidden = true;
  providerChangeSummary.className = "provider-change-summary";
}

export interface ProviderSaveContext {
  providerFromForm: () => Provider;
  getEditingProviderId: () => string | null;
  getCatalogState: () => ProviderCatalogState;
  setProviderEditorDirty: (dirty: boolean) => void;
  refreshProviderEditorControls: () => void;
  closeProviderEditor: (force?: boolean) => Promise<boolean>;
  // 通知由 UI 层装配，features 不直接依赖通知组件。
  notify: (message: string, kind?: "success" | "error") => void;
}

function renderProviderChangeSummary(summary: ProviderChangeSummary): void {
  const providerChangeSummary = element<HTMLElement>("#provider-change-summary");
  providerChangeSummary.replaceChildren();
  providerChangeSummary.hidden = false;
  providerChangeSummary.className = `provider-change-summary${summary.fallbackBlockers.length > 0 ? " blocked" : summary.removedVirtualModels.length > 0 ? " destructive" : ""}`;
  const title = document.createElement("strong");
  title.textContent = summary.fallbackBlockers.length > 0
    ? t("models.cannotSaveTitle")
    : t("models.saveImpact");
  const list = document.createElement("ul");
  const lines = [
    t("models.upstreamChangeSummary", {
      added: summary.addedUpstreamIds.length,
      removed: summary.removedUpstreamIds.length,
    }),
    t("models.virtualChangeSummary", {
      added: summary.addedVirtualModels.length,
      retained: summary.retainedVirtualCount,
      removed: summary.removedVirtualModels.length,
    }),
  ];
  if (summary.unavailableModelIds.length > 0) {
    lines.push(t("models.unavailableChangeSummary", { count: summary.unavailableModelIds.length }));
  }
  for (const blocker of summary.fallbackBlockers) {
    lines.push(t("models.fallbackBlocker", blocker));
  }
  for (const line of lines) {
    const item = document.createElement("li");
    item.textContent = line;
    list.append(item);
  }
  if (summary.removedVirtualModels.length > 0) {
    const removed = document.createElement("details");
    const removedSummary = document.createElement("summary");
    removedSummary.textContent = t("models.inspectRemovedModels");
    const names = document.createElement("p");
    names.textContent = new Intl.ListFormat(getLanguage(), {
      style: "long",
      type: "conjunction",
    }).format(summary.removedVirtualModels.map((model) => model.display_name));
    removed.append(removedSummary, names);
    providerChangeSummary.append(title, list, removed);
  } else {
    providerChangeSummary.append(title, list);
  }
}

async function executeProviderSave(
  plan: ProviderSavePlan,
  context: ProviderSaveContext,
): Promise<void> {
  setActiveProviderTabId(plan.provider.id);

  const currentUpstreamIds = new Set(
    store.config.upstream_models
      .filter((upstream) => upstream.provider_id === plan.provider.id)
      .map((upstream) => upstream.id),
  );
  const connectionResultIds = store.config.virtual_models
    .filter((virtualModel) => currentUpstreamIds.has(virtualModel.upstream_model_id))
    .map((virtualModel) => virtualModel.id);
  await persistProviderConfig(plan.provider.id, plan.nextConfig);
  for (const virtualModelId of connectionResultIds) {
    connectionTestResults.delete(virtualModelId);
  }
  providerTestSessions.delete(plan.provider.id);
  const providerUpstreamIds = new Set(plan.nextConfig.upstream_models
    .filter((upstream) => upstream.provider_id === plan.provider.id)
    .map((upstream) => upstream.id));
  const currentCount = plan.nextConfig.virtual_models.filter(
    (virtualModel) => providerUpstreamIds.has(virtualModel.upstream_model_id),
  ).length;
  context.setProviderEditorDirty(false);
  void context.closeProviderEditor(true);
  context.notify(t("models.providerSaved", {
    action: plan.wasEditing ? t("models.updated") : t("models.added"),
    name: plan.provider.name,
    count: currentCount,
  }));
}

function selectedCatalogModels(catalog: ProviderCatalogState): ProviderCatalogModel[] {
  return catalog.catalogModels.filter((model) => catalog.selectedCatalogModelIds.has(model.id));
}

function providerSelectionError(
  provider: Provider,
  catalog: ProviderCatalogState,
  selectedModels: ProviderCatalogModel[],
): string | null {
  if (selectedModels.length === 0) return t("models.noValidSelectedModels");
  const missingReasoningLevels = selectedModels.find(
    (model) => catalog.catalogReasoningEnabledModelIds.has(model.id)
      && (catalog.catalogReasoningLevelsByModel.get(model.id)?.size ?? 0) === 0
      && !catalog.catalogCustomReasoningByModel.has(model.id)
      && catalog.catalogThinkingBudgetsByModel.get(model.id)?.thinkingBudget == null
      && catalog.catalogThinkingBudgetsByModel.get(model.id)?.minThinkingBudget == null,
  );
  if (missingReasoningLevels) {
    return t("models.reasoningLevelRequired", { name: missingReasoningLevels.displayName });
  }
  const invalidCustomReasoning = selectedModels.find((model) => {
    const value = catalog.catalogCustomReasoningByModel.get(model.id);
    const outputTokenLimit = catalog.catalogTokenLimitsByModel.get(model.id)?.output_token_limit
      ?? model.outputTokenLimit
      ?? null;
    return catalog.catalogReasoningEnabledModelIds.has(model.id)
      && value !== undefined
      && customReasoningMapping(provider.protocol, value, outputTokenLimit) === null;
  });
  if (invalidCustomReasoning) {
    return t("models.invalidReasoningValue", { name: invalidCustomReasoning.displayName });
  }
  return null;
}

function createProviderSavePlan(
  context: ProviderSaveContext,
  provider: Provider,
  catalog: ProviderCatalogState,
): ProviderSavePlan {
  return buildProviderSavePlan({
    currentConfig: store.config,
    provider,
    editingProviderId: context.getEditingProviderId(),
    catalogModels: catalog.catalogModels,
    selectedCatalogModelIds: catalog.selectedCatalogModelIds,
    catalogReasoningLevelsByModel: catalog.catalogReasoningLevelsByModel,
    catalogCustomReasoningByModel: catalog.catalogCustomReasoningByModel,
    catalogThinkingBudgetsByModel: catalog.catalogThinkingBudgetsByModel,
    catalogVisionEnabledModelIds: catalog.catalogVisionEnabledModelIds,
    catalogVideoEnabledModelIds: catalog.catalogVideoEnabledModelIds,
    catalogSupportedMimeTypesByModel: catalog.catalogSupportedMimeTypesByModel,
    catalogToolsEnabledModelIds: catalog.catalogToolsEnabledModelIds,
    catalogReasoningEnabledModelIds: catalog.catalogReasoningEnabledModelIds,
    catalogTokenLimitsByModel: catalog.catalogTokenLimitsByModel,
    changedCatalogTokenLimitModelIds: catalog.changedCatalogTokenLimitModelIds,
    changedCatalogCapabilityModelIds: catalog.changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds: catalog.changedCatalogReasoningModelIds,
    unavailableCatalogModelIds: catalog.unavailableCatalogModelIds,
    createId: () => crypto.randomUUID(),
  });
}

export async function saveProvider(context: ProviderSaveContext): Promise<void> {
  if (pendingProviderSavePlan) {
    const plan = pendingProviderSavePlan;
    pendingProviderSavePlan = null;
    await executeProviderSave(plan, context);
    return;
  }

  const providerForm = element<HTMLFormElement>("#provider-form");
  const catalog = context.getCatalogState();
  if (!providerForm.reportValidity() || catalog.selectedCatalogModelIds.size === 0) return;
  const provider = context.providerFromForm();
  const validationError = providerSelectionError(provider, catalog, selectedCatalogModels(catalog));
  if (validationError) {
    context.notify(validationError, "error");
    return;
  }

  const plan = createProviderSavePlan(context, provider, catalog);
  renderProviderChangeSummary(plan.summary);
  if (plan.summary.fallbackBlockers.length > 0) {
    context.notify(
      t("models.cannotSave", {
        reason: t("models.fallbackBlocker", plan.summary.fallbackBlockers[0]),
      }),
      "error",
    );
    return;
  }
  if (plan.summary.removedVirtualModels.length > 0) {
    pendingProviderSavePlan = plan;
    context.refreshProviderEditorControls();
    context.notify(t("models.confirmRemoval"), "error");
    return;
  }
  await executeProviderSave(plan, context);
}
