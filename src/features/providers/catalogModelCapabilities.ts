import { openReasoningModal } from "../../components/ReasoningModal";
import { t } from "../../i18n";
import type { ConfigurableReasoningLevel, ThinkingBudgetConfig } from "../../types/reasoning";
import {
  catalogReasoningMetadataLabel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import {
  hasMimeTypeCategory,
  normalizeMediaMimeTypes,
  normalizeSupportedMimeTypes,
  supportsVideoInput,
} from "./modelMediaCapabilities";
import type { CatalogModelRowState } from "./catalogModelRowState";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";
import { testProviderModelConnection } from "./providerTesting";

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
  modelId: string,
  label: string,
  enabledModelIds: Set<string>,
  onChange: (enabled: boolean) => void,
  disabled = false,
  title?: string,
): { element: HTMLLabelElement; checkbox: HTMLInputElement } {
  const toggle = document.createElement("label");
  toggle.className = "check-label catalog-capability-toggle";
  if (title) toggle.title = title;
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = enabledModelIds.has(modelId);
  checkbox.disabled = disabled;
  if (title) checkbox.title = title;
  checkbox.addEventListener("change", () => {
    if (checkbox.checked) enabledModelIds.add(modelId);
    else enabledModelIds.delete(modelId);
    onChange(checkbox.checked);
  });
  const copy = document.createElement("span");
  copy.textContent = label;
  toggle.append(checkbox, copy);
  return { element: toggle, checkbox };
}

function selectedMimeTypes(
  modelId: string,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): string[] {
  return normalizeMediaMimeTypes(state.catalogSupportedMimeTypesByModel.get(modelId) ?? [], {
    supportsImages: state.catalogVisionEnabledModelIds.has(modelId),
    supportsVideo: state.catalogVideoEnabledModelIds.has(modelId),
    videoAvailable: supportsVideoInput(context.selectedProtocol()),
  });
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
  const videoAvailable = supportsVideoInput(context.selectedProtocol());
  const markChanged = () => {
    state.changedCatalogCapabilityModelIds.add(model.id);
    context.setProviderEditorDirty(true);
  };
  let refreshMimeTypeInput: () => void = () => undefined;
  const imageToggle = catalogCapabilityToggle(
    model.id,
    t("models.visionInput"),
    state.catalogVisionEnabledModelIds,
    () => {
      refreshMimeTypeInput();
      markChanged();
    },
  );
  const videoToggle = catalogCapabilityToggle(
    model.id,
    t("models.videoInput"),
    state.catalogVideoEnabledModelIds,
    () => {
      refreshMimeTypeInput();
      markChanged();
    },
    !videoAvailable,
    videoAvailable ? undefined : t("models.videoInputUnavailable"),
  );
  const toolsToggle = catalogCapabilityToggle(
    model.id,
    t("models.toolCalling"),
    state.catalogToolsEnabledModelIds,
    () => markChanged(),
  );
  const mimeTypeField = document.createElement("label");
  mimeTypeField.className = "catalog-token-field";
  const mimeTypeLabel = document.createElement("span");
  mimeTypeLabel.textContent = t("models.supportedMimeTypes");
  const mimeTypeInput = document.createElement("input");
  mimeTypeInput.type = "text";
  mimeTypeInput.className = "catalog-token-input";
  mimeTypeInput.size = 34;
  mimeTypeInput.placeholder = t("models.supportedMimeTypesPlaceholder");
  mimeTypeInput.title = videoAvailable
    ? t("models.supportedMimeTypesHint")
    : t("models.supportedMimeTypesImageOnly");
  mimeTypeInput.disabled = !selected;
  refreshMimeTypeInput = () => {
    const mimeTypes = selectedMimeTypes(model.id, context, state);
    state.catalogSupportedMimeTypesByModel.set(model.id, new Set(mimeTypes));
    mimeTypeInput.value = mimeTypes.join(", ");
  };
  refreshMimeTypeInput();
  mimeTypeInput.addEventListener("change", () => {
    const parsedMimeTypes = normalizeSupportedMimeTypes(mimeTypeInput.value.split(/[,\n]+/));
    if (hasMimeTypeCategory(parsedMimeTypes, "image")) {
      state.catalogVisionEnabledModelIds.add(model.id);
    } else {
      state.catalogVisionEnabledModelIds.delete(model.id);
    }
    if (videoAvailable && hasMimeTypeCategory(parsedMimeTypes, "video")) {
      state.catalogVideoEnabledModelIds.add(model.id);
    } else {
      state.catalogVideoEnabledModelIds.delete(model.id);
    }
    state.catalogSupportedMimeTypesByModel.set(model.id, new Set(parsedMimeTypes));
    refreshMimeTypeInput();
    imageToggle.checkbox.checked = state.catalogVisionEnabledModelIds.has(model.id);
    videoToggle.checkbox.checked = state.catalogVideoEnabledModelIds.has(model.id);
    markChanged();
    rerender();
  });
  mimeTypeField.append(mimeTypeLabel, mimeTypeInput);
  capabilities.append(
    imageToggle.element,
    videoToggle.element,
    toolsToggle.element,
    mimeTypeField,
    createReasoningButton(rowState, context, state, rerender),
  );
  if (!selected) {
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = true;
    }
  }
  return capabilities;
}
