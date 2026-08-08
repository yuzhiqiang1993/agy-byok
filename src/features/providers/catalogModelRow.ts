import { t } from "../../i18n";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import { createCatalogModelCapabilities } from "./catalogModelCapabilities";
import { createModelCopy, type CatalogModelSummary } from "./catalogModelSummary";
import { resolveCatalogModelRowState, type CatalogModelRowState } from "./catalogModelRowState";
import { createTokenLimitControls } from "./catalogTokenControls";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";
import { runCatalogModelTests } from "./providerTesting";

function createSelectionControl(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  rerender: () => void,
): { element: HTMLLabelElement; summary: CatalogModelSummary } {
  const { model, selected } = rowState;
  const select = document.createElement("label");
  select.className = "catalog-model-select";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = selected;
  checkbox.addEventListener("change", () => {
    if (checkbox.checked) {
      state.selectedCatalogModelIds.add(model.id);
      state.expandedCatalogModelIds.add(model.id);
    } else {
      state.selectedCatalogModelIds.delete(model.id);
      state.expandedCatalogModelIds.delete(model.id);
    }
    context.setProviderEditorDirty(true);
    rerender();
  });
  const copy = createModelCopy(rowState, state);
  select.append(checkbox, copy.element);
  return { element: select, summary: copy.summary };
}

function createTestArea(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): HTMLDivElement {
  const { model, existingUpstream } = rowState;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary compact-button";
  button.textContent = t("models.testConnectionShort");
  button.title = t("models.testSelectedReasoning");
  const result = document.createElement("span");
  result.className = "catalog-model-test-result";
  result.setAttribute("role", "status");
  button.addEventListener("click", () => {
    runCatalogModelTests({
      button,
      result,
      modelId: model.id,
      model,
      existingUpstream,
      providerFromForm: context.providerFromForm,
      isReasoningEnabled: () => state.catalogReasoningEnabledModelIds.has(model.id),
      selectedReasoningLevels: () => state.catalogReasoningLevelsByModel.get(model.id)
        ?? new Set<ConfigurableReasoningLevel>(),
      runBusy: context.withProviderEditorBusy,
    });
  });
  const area = document.createElement("div");
  area.className = "catalog-model-test-area";
  area.append(button, result);
  return area;
}

function createExpandButton(
  rowState: CatalogModelRowState,
  state: CatalogModelListState,
  rerender: () => void,
): HTMLButtonElement {
  const { model, selected, expanded } = rowState;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "catalog-model-expand";
  button.textContent = t(expanded ? "models.collapseModelSettings" : "models.expandModelSettings");
  button.disabled = !selected;
  button.setAttribute("aria-expanded", String(expanded));
  button.addEventListener("click", () => {
    if (expanded) state.expandedCatalogModelIds.delete(model.id);
    else state.expandedCatalogModelIds.add(model.id);
    rerender();
  });
  return button;
}

function createModelActions(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  refreshSummary: () => void,
  rerender: () => void,
): HTMLDivElement {
  const { model, selected, expanded } = rowState;
  const capabilityGroup = document.createElement("div");
  capabilityGroup.className = "catalog-capability-group";
  const title = document.createElement("span");
  title.className = "catalog-capability-title";
  title.textContent = t("models.capabilityColumn");
  capabilityGroup.append(
    title,
    createCatalogModelCapabilities(rowState, context, state, rerender),
  );

  const actions = document.createElement("div");
  actions.className = "catalog-model-actions";
  actions.hidden = !expanded;
  actions.append(
    createTokenLimitControls(model, selected, selected && expanded, context, state, refreshSummary),
    capabilityGroup,
  );
  return actions;
}

export function createCatalogModelRow(
  model: ProviderCatalogModel,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  rerender: () => void,
): HTMLDivElement {
  const rowState = resolveCatalogModelRowState(model, context, state);
  const { selected, expanded } = rowState;
  const row = document.createElement("div");
  row.className = `catalog-model-row${selected ? " selected" : " unselected"}${expanded ? " expanded" : ""}${state.unavailableCatalogModelIds.has(model.id) ? " unavailable" : ""}`;
  const selection = createSelectionControl(rowState, context, state, rerender);
  const headerActions = document.createElement("div");
  headerActions.className = "catalog-model-header-actions";
  headerActions.append(
    createTestArea(rowState, context, state),
    createExpandButton(rowState, state, rerender),
  );
  const header = document.createElement("div");
  header.className = "catalog-model-header";
  header.append(selection.element, headerActions);
  row.append(
    header,
    createModelActions(rowState, context, state, selection.summary.refreshTokenAndCheckpoint, rerender),
  );
  return row;
}
