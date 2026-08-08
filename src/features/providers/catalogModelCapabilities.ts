import { openReasoningModal } from "../../components/ReasoningModal";
import { t } from "../../i18n";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import {
  catalogReasoningMetadataLabel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import type { CatalogModelRowState } from "./catalogModelRowState";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";
import { testProviderModelConnection } from "./providerTesting";

function applyReasoningSelection(
  modelId: string,
  levels: Set<ConfigurableReasoningLevel>,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): void {
  const previousLevels = state.catalogReasoningLevelsByModel.get(modelId)
    ?? new Set<ConfigurableReasoningLevel>();
  const levelsChanged = previousLevels.size !== levels.size
    || [...previousLevels].some((level) => !levels.has(level));
  if (levels.size > 0) {
    state.catalogReasoningEnabledModelIds.add(modelId);
    state.catalogReasoningLevelsByModel.set(modelId, levels);
  } else {
    state.catalogReasoningEnabledModelIds.delete(modelId);
    state.catalogReasoningLevelsByModel.delete(modelId);
  }
  if (levelsChanged) {
    state.changedCatalogReasoningModelIds.add(modelId);
    context.setProviderEditorDirty(true);
  }
}

function createReasoningButton(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  rerender: () => void,
): HTMLButtonElement {
  const {
    model,
    selected,
    existingUpstream,
    selectedReasoningLevels,
    availableReasoningLevels,
    reasoningEnabled,
  } = rowState;
  const button = document.createElement("button");
  button.type = "button";
  button.className = `catalog-reasoning-trigger${reasoningEnabled ? " active" : ""}`;
  const levelsSummary = reasoningEnabled && selectedReasoningLevels
    ? sortReasoningLevels(selectedReasoningLevels).map(reasoningLevelLabel).join(" · ")
    : "";
  button.textContent = reasoningEnabled
    ? t("models.reasoningSummary", { levels: levelsSummary })
    : t("models.configureReasoning");
  button.title = catalogReasoningMetadataLabel(model) ?? t("models.configureReasoningHint");
  button.disabled = !selected || availableReasoningLevels.length === 0;
  button.addEventListener("click", () => {
    openReasoningModal(model, {
      providerProtocol: context.selectedProtocol(),
      existingUpstream,
      currentLevels: selectedReasoningLevels ?? new Set<ConfigurableReasoningLevel>(),
      providerFromForm: context.providerFromForm,
      testProviderModelConnection,
      runBusy: context.withProviderEditorBusy,
      onConfirm: (modelId, levels) => {
        applyReasoningSelection(modelId, levels, context, state);
        rerender();
      },
    });
  });
  return button;
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

export function createCatalogModelCapabilities(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  rerender: () => void,
): HTMLDivElement {
  const { model, selected } = rowState;
  const capabilities = document.createElement("div");
  capabilities.className = "catalog-model-capabilities";
  const markChanged = () => {
    state.changedCatalogCapabilityModelIds.add(model.id);
    context.setProviderEditorDirty(true);
  };
  capabilities.append(
    catalogCapabilityToggle(model.id, t("models.visionInput"), state.catalogVisionEnabledModelIds, markChanged),
    catalogCapabilityToggle(model.id, t("models.toolCalling"), state.catalogToolsEnabledModelIds, markChanged),
    createReasoningButton(rowState, context, state, rerender),
  );
  for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
    input.disabled = !selected;
  }
  return capabilities;
}
