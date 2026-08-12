import { openReasoningModal } from "../../components/ReasoningModal";
import { openMultimodalModal } from "../../components/MultimodalModal";
import { t } from "../../i18n";
import type { ConfigurableReasoningLevel, ThinkingBudgetConfig } from "../../types/reasoning";
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
import {
  MULTIMODAL_INPUT_MODALITIES,
  normalizeSelectedInputMimeTypes,
  type MultimodalInputModality,
} from "./modelMediaCapabilities";

function applyReasoningSelection(
  modelId: string,
  levels: Set<ConfigurableReasoningLevel>,
  budgets: ThinkingBudgetConfig,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): void {
  const previousLevels = state.catalogReasoningLevelsByModel.get(modelId)
    ?? new Set<ConfigurableReasoningLevel>();
  const levelsChanged = previousLevels.size !== levels.size
    || [...previousLevels].some((level) => !levels.has(level));
  const previousBudgets = state.catalogThinkingBudgetsByModel.get(modelId) ?? {
    thinkingBudget: null,
    minThinkingBudget: null,
  };
  const budgetsChanged = previousBudgets.thinkingBudget !== budgets.thinkingBudget
    || previousBudgets.minThinkingBudget !== budgets.minThinkingBudget;
  const reasoningEnabled = levels.size > 0
    || budgets.thinkingBudget != null
    || budgets.minThinkingBudget != null;
  if (reasoningEnabled) {
    state.catalogReasoningEnabledModelIds.add(modelId);
    state.catalogReasoningLevelsByModel.set(modelId, levels);
  } else {
    state.catalogReasoningEnabledModelIds.delete(modelId);
    state.catalogReasoningLevelsByModel.delete(modelId);
  }
  state.catalogThinkingBudgetsByModel.set(modelId, budgets);
  if (levelsChanged || budgetsChanged) {
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
  const thinkingBudgets = state.catalogThinkingBudgetsByModel.get(model.id) ?? {
    thinkingBudget: null,
    minThinkingBudget: null,
  };
  const outputTokenLimit = state.catalogTokenLimitsByModel.get(model.id)?.output_token_limit
    ?? model.outputTokenLimit
    ?? null;
  const button = document.createElement("button");
  button.type = "button";
  button.className = `catalog-reasoning-trigger${reasoningEnabled ? " active" : ""}`;
  const levelsSummary = reasoningEnabled && selectedReasoningLevels
    ? sortReasoningLevels(selectedReasoningLevels).map(reasoningLevelLabel).join(" · ")
    : "";
  button.textContent = reasoningEnabled
    ? levelsSummary
      ? t("models.reasoningSummary", { levels: levelsSummary })
      : t("models.reasoningBudgetSummary", {
          budget: thinkingBudgets.thinkingBudget ?? t("models.reasoningDynamicBudget"),
        })
    : t("models.configureReasoning");
  button.title = catalogReasoningMetadataLabel(model) ?? t("models.configureReasoningHint");
  button.disabled = !selected || (
    availableReasoningLevels.length === 0
    && model.reasoning?.supported === false
    && thinkingBudgets.thinkingBudget == null
    && thinkingBudgets.minThinkingBudget == null
  );
  button.addEventListener("click", () => {
    openReasoningModal(model, {
      providerProtocol: context.selectedProtocol(),
      existingUpstream,
      outputTokenLimit,
      currentLevels: selectedReasoningLevels ?? new Set<ConfigurableReasoningLevel>(),
      currentThinkingBudgets: thinkingBudgets,
      providerFromForm: context.providerFromForm,
      testProviderModelConnection,
      runBusy: context.withProviderEditorBusy,
      onConfirm: (modelId, levels, budgets) => {
        applyReasoningSelection(modelId, levels, budgets, context, state);
        rerender();
      },
    });
  });
  return button;
}

function catalogCapabilityToggle(
  label: string,
  checked: boolean,
  onChange: (enabled: boolean) => void,
): { element: HTMLLabelElement; checkbox: HTMLInputElement } {
  const toggle = document.createElement("label");
  toggle.className = "check-label catalog-capability-toggle";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = checked;
  checkbox.addEventListener("change", () => {
    onChange(checkbox.checked);
  });
  const copy = document.createElement("span");
  copy.textContent = label;
  toggle.append(checkbox, copy);
  return { element: toggle, checkbox };
}

function selectedMultimodalModalities(
  state: CatalogModelListState,
  modelId: string,
): Set<MultimodalInputModality> {
  const modalities = new Set<MultimodalInputModality>();
  if (state.catalogImageInputModelIds.has(modelId)) modalities.add("image");
  if (state.catalogDocumentInputModelIds.has(modelId)) modalities.add("document");
  if (state.catalogAudioInputModelIds.has(modelId)) modalities.add("audio");
  if (state.catalogVideoInputModelIds.has(modelId)) modalities.add("video");
  return modalities;
}

function applyMultimodalModalities(
  state: CatalogModelListState,
  modelId: string,
  modalities: ReadonlySet<MultimodalInputModality>,
): void {
  for (const [modality, modelIds] of [
    ["image", state.catalogImageInputModelIds],
    ["document", state.catalogDocumentInputModelIds],
    ["audio", state.catalogAudioInputModelIds],
    ["video", state.catalogVideoInputModelIds],
  ] as const) {
    if (modalities.has(modality)) modelIds.add(modelId);
    else modelIds.delete(modelId);
  }
  const currentMimeTypes = state.catalogInputMimeTypesByModel.get(modelId) ?? new Set<string>();
  state.catalogInputMimeTypesByModel.set(
    modelId,
    new Set(normalizeSelectedInputMimeTypes(currentMimeTypes, modalities)),
  );
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
  const currentModalities = selectedMultimodalModalities(state, model.id);
  const multimodalControl = document.createElement("span");
  multimodalControl.className = "catalog-multimodal-control";
  const multimodalToggle = catalogCapabilityToggle(
    t("models.multimodalInput"),
    currentModalities.size > 0,
    (enabled) => {
      applyMultimodalModalities(
        state,
        model.id,
        enabled ? new Set(MULTIMODAL_INPUT_MODALITIES) : new Set(),
      );
      markChanged();
      rerender();
    },
  );
  const editMultimodalButton = document.createElement("button");
  editMultimodalButton.type = "button";
  editMultimodalButton.className = "catalog-multimodal-edit";
  editMultimodalButton.textContent = t("models.editMultimodal");
  editMultimodalButton.disabled = currentModalities.size === 0;
  editMultimodalButton.addEventListener("click", () => {
    openMultimodalModal(model, {
      currentModalities,
      onConfirm: (modalities) => {
        applyMultimodalModalities(state, model.id, modalities);
        markChanged();
        rerender();
      },
    });
  });
  multimodalControl.append(multimodalToggle.element, editMultimodalButton);
  const toolsToggle = catalogCapabilityToggle(
    t("models.toolCalling"),
    state.catalogToolsEnabledModelIds.has(model.id),
    (enabled) => {
      if (enabled) state.catalogToolsEnabledModelIds.add(model.id);
      else state.catalogToolsEnabledModelIds.delete(model.id);
      markChanged();
      rerender();
    },
  );

  capabilities.append(
    multimodalControl,
    toolsToggle.element,
    createReasoningButton(rowState, context, state, rerender),
  );
  if (!selected) {
    for (const control of capabilities.querySelectorAll<HTMLInputElement | HTMLButtonElement>(
      "input, button",
    )) {
      control.disabled = true;
    }
  }
  return capabilities;
}
