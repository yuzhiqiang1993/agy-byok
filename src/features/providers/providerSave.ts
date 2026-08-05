import type { ProviderChangeSummary, ProviderSavePlan } from "../../types/proxy";
import type { Provider } from "../../types/config";
import { store } from "../../store/appStore";
import { persistConfig } from "../../controllers/providerController";
import {
  connectionTestResults,
  providerTestSessions,
  setActiveProviderTabId,
} from "./providerState";
import { buildProviderSavePlan } from "./providerPlan";
import type { ProviderCatalogState } from "./providerCatalog";
import { element } from "../../utils/domUtils";
import { showNotice } from "../../components/NoticeBar";
import { customReasoningMapping } from "../../utils/reasoningUtils";
import { t } from "../../i18n";

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
  if (summary.legacyModelIds.length > 0) {
    lines.push(t("models.legacyChangeSummary", { count: summary.legacyModelIds.length }));
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
    names.textContent = summary.removedVirtualModels.map((model) => model.display_name).join("、");
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
  for (const virtualModel of store.config.virtual_models) {
    if (currentUpstreamIds.has(virtualModel.upstream_model_id)) {
      connectionTestResults.delete(virtualModel.id);
    }
  }
  providerTestSessions.delete(plan.provider.id);
  await persistConfig(plan.nextConfig);
  const currentCount = plan.nextConfig.virtual_models.filter((virtualModel) => {
    const upstream = plan.nextConfig.upstream_models.find(
      (item) => item.id === virtualModel.upstream_model_id,
    );
    return upstream?.provider_id === plan.provider.id;
  }).length;
  context.setProviderEditorDirty(false);
  void context.closeProviderEditor(true);
  showNotice(t("models.providerSaved", {
    action: plan.wasEditing ? t("models.updated") : t("models.added"),
    name: plan.provider.name,
    count: currentCount,
  }));
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
  const selectedModels = catalog.catalogModels.filter((model) => catalog.selectedCatalogModelIds.has(model.id));
  if (selectedModels.length === 0) {
    showNotice(t("models.noValidSelectedModels"), "error");
    return;
  }

  const missingReasoningLevels = selectedModels.find(
    (model) => catalog.catalogReasoningEnabledModelIds.has(model.id)
      && (catalog.catalogReasoningLevelsByModel.get(model.id)?.size ?? 0) === 0
      && !catalog.catalogCustomReasoningByModel.has(model.id),
  );
  if (missingReasoningLevels) {
    showNotice(t("models.reasoningLevelRequired", { name: missingReasoningLevels.displayName }), "error");
    return;
  }

  const invalidCustomReasoning = selectedModels.find((model) => {
    const value = catalog.catalogCustomReasoningByModel.get(model.id);
    return catalog.catalogReasoningEnabledModelIds.has(model.id)
      && value !== undefined
      && customReasoningMapping(provider.protocol, value) === null;
  });
  if (invalidCustomReasoning) {
    showNotice(t("models.invalidReasoningValue", { name: invalidCustomReasoning.displayName }), "error");
    return;
  }

  const plan = buildProviderSavePlan({
    currentConfig: store.config,
    provider,
    editingProviderId: context.getEditingProviderId(),
    catalogModels: catalog.catalogModels,
    selectedCatalogModelIds: catalog.selectedCatalogModelIds,
    catalogReasoningLevelsByModel: catalog.catalogReasoningLevelsByModel,
    catalogCustomReasoningByModel: catalog.catalogCustomReasoningByModel,
    catalogVisionEnabledModelIds: catalog.catalogVisionEnabledModelIds,
    catalogToolsEnabledModelIds: catalog.catalogToolsEnabledModelIds,
    catalogReasoningEnabledModelIds: catalog.catalogReasoningEnabledModelIds,
    catalogTokenLimitsByModel: catalog.catalogTokenLimitsByModel,
    changedCatalogTokenLimitModelIds: catalog.changedCatalogTokenLimitModelIds,
    changedCatalogCapabilityModelIds: catalog.changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds: catalog.changedCatalogReasoningModelIds,
    legacyCatalogModelIds: catalog.legacyCatalogModelIds,
    createId: () => crypto.randomUUID(),
  });
  renderProviderChangeSummary(plan.summary);
  if (plan.summary.fallbackBlockers.length > 0) {
    showNotice(
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
    showNotice(t("models.confirmRemoval"), "error");
    return;
  }
  await executeProviderSave(plan, context);
}
