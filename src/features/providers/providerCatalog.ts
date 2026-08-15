import { fetchProviderCatalog as fetchProviderCatalogCommand } from "../../controllers/providerController";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import { element } from "../../utils/domUtils";
import { renderCatalogModelList } from "./catalogModelList";
import {
  emptyCatalogState,
  projectLoadedCatalogState,
  type InternalProviderCatalogState,
} from "./catalogStateProjector";
import { isLikelyImageModel } from "./modelRoleClassifier";
import type {
  ProviderCatalogContext,
  ProviderCatalogState,
} from "./providerCatalogTypes";

export { isLikelyImageModel };
export type { ProviderCatalogContext, ProviderCatalogState } from "./providerCatalogTypes";

let catalogState: InternalProviderCatalogState = emptyCatalogState();

export function getProviderCatalogState(): ProviderCatalogState {
  return catalogState;
}

export function setCatalogModelSelection(modelIds: Iterable<string>, selected: boolean): void {
  for (const modelId of modelIds) {
    if (selected) catalogState.selectedCatalogModelIds.add(modelId);
    else catalogState.selectedCatalogModelIds.delete(modelId);
  }
}

export function renderCatalogStatus(): void {
  const status = element<HTMLElement>("#catalog-status");
  status.textContent = catalogState.source === "configured"
    ? t("models.configuredModelLoaded")
    : catalogState.fetchedCount === 0
    ? t("models.fetching")
    : catalogState.hasUnavailableModels
      ? t("models.catalogFetchedWithUnavailable", {
          count: catalogState.fetchedCount,
          unavailable: catalogState.unavailableCatalogModelIds.size,
        })
      : t("models.catalogFetched", { count: catalogState.fetchedCount });
}

function showProviderConfigStep(): void {
  const configStep = element<HTMLElement>("#provider-step-config");
  configStep.hidden = false;
  configStep.classList.add("active");
  const catalogResults = element<HTMLElement>("#catalog-results");
  catalogResults.hidden = true;
  catalogResults.classList.remove("active");
}

function showCatalogResultsStep(): void {
  const configStep = element<HTMLElement>("#provider-step-config");
  configStep.classList.remove("active");
  configStep.hidden = true;
  const catalogResults = element<HTMLElement>("#catalog-results");
  catalogResults.hidden = false;
  catalogResults.classList.add("active");
}

export function resetCatalogResults(): void {
  catalogState = emptyCatalogState();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();
  showProviderConfigStep();
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  element<HTMLButtonElement>("#save-provider").disabled = true;
  renderCatalogStatus();
}

export async function fetchProviderCatalog(context: ProviderCatalogContext): Promise<void> {
  if (!element<HTMLFormElement>("#provider-form").reportValidity()) return;
  context.invalidatePendingProviderSave();
  context.refreshProviderEditorControls();
  const provider = context.providerFromForm();
  const fetched = await fetchProviderCatalogCommand(provider);
  const editingProviderId = context.getEditingProviderId();
  const existingUpstreams = editingProviderId
    ? store.config.upstream_models.filter((upstream) => upstream.provider_id === editingProviderId)
    : [];
  catalogState = projectLoadedCatalogState(fetched, existingUpstreams, provider.protocol);
  showCatalogResultsStep();
  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>(".provider-modal-body").scrollTop = 0;
}

export function loadConfiguredProviderCatalog(
  context: ProviderCatalogContext,
  providerId: string,
): void {
  const existingUpstreams = store.config.upstream_models.filter(
    (upstream) => upstream.provider_id === providerId,
  );
  // 单模型编辑只读取当前持久化配置，不静默发起目录请求。
  const configuredModels: ProviderCatalogModel[] = existingUpstreams.map((upstream) => ({
    id: upstream.upstream_model_id,
    displayName: upstream.display_name,
  }));
  catalogState = projectLoadedCatalogState(
    configuredModels,
    existingUpstreams,
    context.selectedProtocol(),
    "configured",
  );
  showCatalogResultsStep();
  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>(".provider-modal-body").scrollTop = 0;
}

export function updateCatalogSelection(context: ProviderCatalogContext): void {
  const count = catalogState.selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent = count > 0
    ? t("models.selectedModels", { count })
    : t("models.noModelSelected");
  context.refreshProviderEditorControls();
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const focusedModelId = context.getFocusedCatalogModelId();
  const visibleIds = catalogState.catalogModels
    .filter((model) => (
      (!focusedModelId || model.id === focusedModelId)
      && `${model.displayName} ${model.id}`.toLowerCase().includes(query)
    ))
    .map((model) => model.id);
  const selectAll = element<HTMLInputElement>("#select-all-models");
  selectAll.checked = visibleIds.length > 0
    && visibleIds.every((id) => catalogState.selectedCatalogModelIds.has(id));
  selectAll.indeterminate = visibleIds.some((id) => catalogState.selectedCatalogModelIds.has(id))
    && !selectAll.checked;
}

export function renderCatalogModels(context: ProviderCatalogContext): void {
  renderCatalogModelList(context, catalogState, () => updateCatalogSelection(context));
}
