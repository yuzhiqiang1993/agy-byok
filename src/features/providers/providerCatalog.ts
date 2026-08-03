import type { ProviderCatalogModel } from "../../types/catalog";
import type { Provider, ProviderProtocol } from "../../types/config";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import { store } from "../../store/appStore";
import { fetchProviderCatalog as fetchProviderCatalogCommand } from "../../controllers/providerController";
import { element } from "../../utils/domUtils";
import {
  catalogReasoningMetadataLabel,
  customReasoningValueFromUpstream,
  reasoningLevelLabel,
  reasoningLevelsForVirtualModels,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import { openReasoningModal } from "../../components/ReasoningModal";
import { t } from "../../i18n";
import { runCatalogModelTests, testProviderModelConnection } from "./providerTesting";

export let catalogModels: ProviderCatalogModel[] = [];
export let selectedCatalogModelIds = new Set<string>();
export let catalogReasoningLevelsByModel = new Map<string, Set<ConfigurableReasoningLevel>>();
export let catalogCustomReasoningByModel = new Map<string, string>();
export let catalogVisionEnabledModelIds = new Set<string>();
export let catalogToolsEnabledModelIds = new Set<string>();
export let catalogReasoningEnabledModelIds = new Set<string>();
export let changedCatalogCapabilityModelIds = new Set<string>();
export let changedCatalogReasoningModelIds = new Set<string>();
export let legacyCatalogModelIds = new Set<string>();
let catalogFetchedCount = 0;
let catalogStatusHasLegacy = false;

export interface ProviderCatalogState {
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogVisionEnabledModelIds: ReadonlySet<string>;
  catalogToolsEnabledModelIds: ReadonlySet<string>;
  catalogReasoningEnabledModelIds: ReadonlySet<string>;
  changedCatalogCapabilityModelIds: ReadonlySet<string>;
  changedCatalogReasoningModelIds: ReadonlySet<string>;
  legacyCatalogModelIds: ReadonlySet<string>;
}

export function getProviderCatalogState(): ProviderCatalogState {
  return {
    catalogModels,
    selectedCatalogModelIds,
    catalogReasoningLevelsByModel,
    catalogCustomReasoningByModel,
    catalogVisionEnabledModelIds,
    catalogToolsEnabledModelIds,
    catalogReasoningEnabledModelIds,
    changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds,
    legacyCatalogModelIds,
  };
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

export function renderCatalogStatus(): void {
  const status = element<HTMLElement>("#catalog-status");
  status.textContent = catalogFetchedCount === 0
    ? t("models.fetching")
    : catalogStatusHasLegacy
      ? t("models.catalogFetchedWithLegacy", {
          count: catalogFetchedCount,
          legacy: legacyCatalogModelIds.size,
        })
      : t("models.catalogFetched", { count: catalogFetchedCount });
}

export function resetCatalogResults(): void {
  catalogFetchedCount = 0;
  catalogStatusHasLegacy = false;
  catalogModels = [];
  selectedCatalogModelIds = new Set();
  catalogReasoningLevelsByModel = new Map();
  catalogCustomReasoningByModel = new Map();
  catalogVisionEnabledModelIds = new Set();
  catalogToolsEnabledModelIds = new Set();
  catalogReasoningEnabledModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();
  element<HTMLElement>("#catalog-results").hidden = true;
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  element<HTMLButtonElement>("#save-provider").disabled = true;
  renderCatalogStatus();
}

export async function fetchProviderCatalog(context: ProviderCatalogContext): Promise<void> {
  const providerForm = element<HTMLFormElement>("#provider-form");
  if (!providerForm.reportValidity()) return;
  context.invalidatePendingProviderSave();
  context.refreshProviderEditorControls();
  const provider = context.providerFromForm();
  const fetched = await fetchProviderCatalogCommand(provider);
  const fetchedIds = new Set(fetched.map((model) => model.id));
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const editingProviderId = context.getEditingProviderId();
  const existingUpstreams = editingProviderId
    ? store.config.upstream_models.filter((item) => item.provider_id === editingProviderId)
    : [];
  legacyCatalogModelIds = new Set(
    existingUpstreams
      .filter((upstream) => !fetchedIds.has(upstream.upstream_model_id))
      .map((upstream) => upstream.upstream_model_id),
  );
  for (const upstream of existingUpstreams) {
    if (!byId.has(upstream.upstream_model_id)) {
      byId.set(upstream.upstream_model_id, {
        id: upstream.upstream_model_id,
        displayName: upstream.display_name,
      });
    }
  }
  catalogModels = [...byId.values()];
  selectedCatalogModelIds = new Set(
    existingUpstreams.map((item) => item.upstream_model_id),
  );
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  const existingUpstreamsByModelId = new Map(
    existingUpstreams.map((upstream) => [upstream.upstream_model_id, upstream]),
  );
  catalogVisionEnabledModelIds = new Set(
    catalogModels
      .filter((model) => existingUpstreamsByModelId.get(model.id)?.capabilities.vision ?? true)
      .map((model) => model.id),
  );
  catalogToolsEnabledModelIds = new Set(
    catalogModels
      .filter((model) => existingUpstreamsByModelId.get(model.id)?.capabilities.tools ?? true)
      .map((model) => model.id),
  );
  catalogReasoningEnabledModelIds = new Set(
    catalogModels
      .filter((model) => {
        const upstream = existingUpstreamsByModelId.get(model.id);
        return upstream
          ? Object.keys(upstream.capabilities.reasoning.levels).length > 0
          : false;
      })
      .map((model) => model.id),
  );
  catalogReasoningLevelsByModel = new Map(catalogModels.map((model) => {
    const upstream = existingUpstreamsByModelId.get(model.id);
    if (!upstream) return [model.id, new Set<ConfigurableReasoningLevel>()];
    const virtualModels = store.config.virtual_models.filter(
      (item) => item.upstream_model_id === upstream.id,
    );
    return [model.id, reasoningLevelsForVirtualModels(provider.protocol, virtualModels)];
  }));
  catalogCustomReasoningByModel = new Map(
    catalogModels.flatMap((model) => {
      const upstream = existingUpstreamsByModelId.get(model.id);
      const value = upstream ? customReasoningValueFromUpstream(upstream) : null;
      return value ? [[model.id, value] as const] : [];
    }),
  );
  catalogFetchedCount = fetched.length;
  catalogStatusHasLegacy = legacyCatalogModelIds.size > 0;
  element<HTMLElement>("#catalog-results").hidden = false;
  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>("#catalog-results").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

export function updateCatalogSelection(context: ProviderCatalogContext): void {
  const count = selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent = count > 0
    ? t("models.selectedModels", { count })
    : t("models.noModelSelected");
  context.refreshProviderEditorControls();
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleIds = catalogModels
    .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
    .map((model) => model.id);
  const selectAll = element<HTMLInputElement>("#select-all-models");
  selectAll.checked = visibleIds.length > 0
    && visibleIds.every((id) => selectedCatalogModelIds.has(id));
  selectAll.indeterminate = visibleIds.some((id) => selectedCatalogModelIds.has(id))
    && !selectAll.checked;
}

function catalogCapabilityToggle(
  modelId: string,
  label: string,
  enabledModelIds: Set<string>,
  onChange: () => void,
): HTMLLabelElement {
  const toggle = document.createElement("label");
  toggle.className = "check-label catalog-capability-toggle";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = enabledModelIds.has(modelId);
  checkbox.addEventListener("change", () => {
    if (checkbox.checked) enabledModelIds.add(modelId);
    else enabledModelIds.delete(modelId);
    onChange();
  });
  const copy = document.createElement("span");
  copy.textContent = label;
  toggle.append(checkbox, copy);
  return toggle;
}

export function renderCatalogModels(context: ProviderCatalogContext): void {
  const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    const selected = selectedCatalogModelIds.has(model.id);
    const editingProviderId = context.getEditingProviderId();
    const existingUpstream = editingProviderId
      ? store.config.upstream_models.find(
          (item) => item.provider_id === editingProviderId && item.upstream_model_id === model.id,
        )
      : undefined;
    row.className = `catalog-model-row${selected ? "" : " unselected"}${legacyCatalogModelIds.has(model.id) ? " legacy" : ""}`;
    const select = document.createElement("label");
    select.className = "catalog-model-select";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selected;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedCatalogModelIds.add(model.id);
      else selectedCatalogModelIds.delete(model.id);
      context.setProviderEditorDirty(true);
      renderCatalogModels(context);
    });
    const copy = document.createElement("span");
    const name = document.createElement("strong");
    name.textContent = model.displayName;
    const id = document.createElement("code");
    id.textContent = model.id;
    copy.append(name);
    if (legacyCatalogModelIds.has(model.id)) {
      const legacy = document.createElement("span");
      legacy.className = "legacy-badge";
      legacy.textContent = t("models.currentCatalogMissing");
      legacy.title = t("models.currentCatalogMissingHint");
      copy.append(legacy);
    }
    copy.append(id);
    const reasoningMetadataLabel = catalogReasoningMetadataLabel(model);
    if (reasoningMetadataLabel) {
      const reasoningHint = document.createElement("span");
      reasoningHint.className = `catalog-reasoning-hint${model.reasoning?.supported === false ? " unsupported" : ""}`;
      reasoningHint.textContent = reasoningMetadataLabel;
      copy.append(reasoningHint);
    }
    select.append(checkbox, copy);

    const capabilities = document.createElement("div");
    capabilities.className = "catalog-model-capabilities";
    const selectedLevels = catalogReasoningLevelsByModel.get(model.id);
    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id) && (selectedLevels?.size ?? 0) > 0;
    const reasoningBtn = document.createElement("button");
    reasoningBtn.type = "button";
    reasoningBtn.className = `catalog-reasoning-trigger${reasoningEnabled ? " active" : ""}`;
    const reasoningLevelsSummary = reasoningEnabled
      ? sortReasoningLevels(selectedLevels!).map(reasoningLevelLabel).join(" · ")
      : "";
    reasoningBtn.textContent = reasoningEnabled
      ? t("models.reasoningSummary", { levels: reasoningLevelsSummary })
      : t("models.configureReasoning");
    const reasoningToggleLabel = catalogReasoningMetadataLabel(model);
    reasoningBtn.title = reasoningToggleLabel ?? t("models.configureReasoningHint");
    reasoningBtn.disabled = !selected || (model.reasoning?.supported === false && !existingUpstream);
    reasoningBtn.addEventListener("click", () => {
      openReasoningModal(model, {
        providerProtocol: context.selectedProtocol(),
        existingUpstream,
        currentLevels: selectedLevels ?? new Set<ConfigurableReasoningLevel>(),
        providerFromForm: context.providerFromForm,
        testProviderModelConnection,
        runBusy: context.withProviderEditorBusy,
        onConfirm: (modelId, levels) => {
          if (levels.size > 0) {
            catalogReasoningEnabledModelIds.add(modelId);
            catalogReasoningLevelsByModel.set(modelId, levels);
          } else {
            catalogReasoningEnabledModelIds.delete(modelId);
            catalogReasoningLevelsByModel.delete(modelId);
          }
          changedCatalogReasoningModelIds.add(modelId);
          context.setProviderEditorDirty(true);
          renderCatalogModels(context);
        },
      });
    });

    capabilities.append(
      catalogCapabilityToggle(model.id, t("models.visionInput"), catalogVisionEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        context.setProviderEditorDirty(true);
      }),
      catalogCapabilityToggle(model.id, t("models.toolCalling"), catalogToolsEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        context.setProviderEditorDirty(true);
      }),
      reasoningBtn,
    );
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = !selected;
    }

    const test = document.createElement("button");
    test.type = "button";
    test.className = "secondary compact-button";
    test.textContent = t("models.testConnectionShort");
    test.title = t("models.testSelectedReasoning");
    const result = document.createElement("span");
    result.className = "catalog-model-test-result";
    result.setAttribute("role", "status");
    test.addEventListener("click", () => {
      runCatalogModelTests({
        button: test,
        result,
        modelId: model.id,
        providerFromForm: context.providerFromForm,
        isReasoningEnabled: () => catalogReasoningEnabledModelIds.has(model.id),
        selectedReasoningLevels: () => catalogReasoningLevelsByModel.get(model.id) ?? new Set<ConfigurableReasoningLevel>(),
        runBusy: context.withProviderEditorBusy,
      });
    });
    const testArea = document.createElement("div");
    testArea.className = "catalog-model-test-area";
    testArea.append(test, result);
    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.append(capabilities);
    actions.append(testArea);
    row.append(select, actions);
    catalogModelList.append(row);
  }

  if (visibleModels.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state compact-empty";
    empty.textContent = t("models.noMatchingModels");
    catalogModelList.append(empty);
  }
  updateCatalogSelection(context);
}
