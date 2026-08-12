import { openReasoningModal } from "../../components/ReasoningModal";
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
import { supportsInputModality } from "./modelMediaCapabilities";

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
  const imageToggle = catalogCapabilityToggle(
    model.id,
    t("models.visionInput"),
    state.catalogImageInputModelIds,
    () => {
      markChanged();
      rerender();
    },
  );
  const audioAvailable = supportsInputModality(context.selectedProtocol(), "audio");
  const audioToggle = catalogCapabilityToggle(
    model.id,
    t("models.audioInput"),
    state.catalogAudioInputModelIds,
    () => {
      markChanged();
      rerender();
    },
    !audioAvailable,
    audioAvailable ? undefined : t("models.adapterMediaUnsupported"),
  );
  const videoAvailable = supportsInputModality(context.selectedProtocol(), "video");
  const videoToggle = catalogCapabilityToggle(
    model.id,
    t("models.videoInput"),
    state.catalogVideoInputModelIds,
    () => {
      markChanged();
      rerender();
    },
    !videoAvailable,
    videoAvailable ? undefined : t("models.adapterMediaUnsupported"),
  );
  const documentToggle = catalogCapabilityToggle(
    model.id,
    t("models.documentInput"),
    state.catalogDocumentInputModelIds,
    () => {
      markChanged();
      rerender();
    },
  );
  const toolsToggle = catalogCapabilityToggle(
    model.id,
    t("models.toolCalling"),
    state.catalogToolsEnabledModelIds,
    () => {
      markChanged();
      rerender();
    },
  );

  capabilities.append(
    imageToggle.element,
    audioToggle.element,
    videoToggle.element,
    documentToggle.element,
    toolsToggle.element,
    createReasoningButton(rowState, context, state, rerender),
  );
  if (!selected) {
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = true;
    }
  }
  return capabilities;
}
