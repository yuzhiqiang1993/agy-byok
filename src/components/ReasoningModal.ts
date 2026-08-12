import { t, subscribeLanguage, isTranslationKey } from "../i18n";
import type { ProviderCatalogModel } from "../types/catalog";
import type { Provider, ProviderProtocol, UpstreamModel } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type {
  ConfigurableReasoningLevel,
  ReasoningMapping,
  ThinkingBudgetConfig,
} from "../types/reasoning";
import {
  reasoningConfigurationSource,
  reasoningMappingSource,
  catalogReasoningLevelsForModel,
  resolveReasoningMappingForModel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../utils/reasoningUtils";
import { connectionTestErrorMessage } from "../utils/connectionTestUtils";
import { createModal, type ModalInstance } from "./common/Modal";

interface ReasoningModalContext {
  providerProtocol: ProviderProtocol;
  existingUpstream?: UpstreamModel;
  outputTokenLimit: number | null;
  currentLevels: ReadonlySet<ConfigurableReasoningLevel>;
  currentThinkingBudgets: ThinkingBudgetConfig;
  providerFromForm: () => Provider;
  testProviderModelConnection: (
    provider: Provider,
    upstreamModelId: string,
    reasoningLevel: ConfigurableReasoningLevel,
    customReasoningValue: string | null,
    reasoningMapping: ReasoningMapping | null,
  ) => Promise<ModelConnectionTestResult>;
  runBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
  onConfirm: (
    modelId: string,
    levels: Set<ConfigurableReasoningLevel>,
    budgets: ThinkingBudgetConfig,
  ) => void;
}

let activeReasoningModel: ProviderCatalogModel | null = null;
let activeContext: ReasoningModalContext | null = null;
let currentModal: ModalInstance | null = null;

// Keep track of active DOM elements for i18n updates if language changes while open
let activeLabels: HTMLSpanElement[] = [];
let activeTestButtons: HTMLButtonElement[] = [];
let activeReadOnlyNote: HTMLElement | null = null;
let activeBudgetLabels: HTMLElement[] = [];
let activeSourceLabels: HTMLElement[] = [];

subscribeLanguage(() => {
  if (!activeReasoningModel || !activeContext || !currentModal) return;
  
  const supportedLevels = catalogReasoningLevelsForModel(
    activeReasoningModel,
    activeContext.providerProtocol,
    activeContext.existingUpstream,
    activeContext.outputTokenLimit,
  );
  
  activeLabels.forEach((label, index) => {
    const level = supportedLevels[index];
    if (level) label.textContent = reasoningLevelLabel(level);
  });
  
  activeTestButtons.forEach((button) => {
    button.textContent = t("models.testConnection");
  });
  
  if (activeReadOnlyNote) {
    activeReadOnlyNote.textContent = t("models.reasoningLevelsReadOnly");
  }
  
  activeBudgetLabels.forEach((label) => {
    const key = label.dataset.reasoningBudgetLabel;
    if (key && isTranslationKey(key)) label.textContent = t(key);
  });

  activeSourceLabels.forEach((label) => {
    const key = label.dataset.reasoningSourceLabel;
    if (key && isTranslationKey(key)) label.textContent = t(key);
  });
});

function configurationSourceLabelKey(
  model: ProviderCatalogModel,
  existingUpstream?: UpstreamModel,
): "models.reasoningSourceCatalog" | "models.reasoningSourceCatalogAdaptive" | "models.reasoningSourceCatalogCapability" | "models.reasoningSourceConfigured" | "models.reasoningSourceSuggested" {
  switch (reasoningConfigurationSource(model, existingUpstream)) {
    case "catalog": return "models.reasoningSourceCatalog";
    case "catalog_adaptive": return "models.reasoningSourceCatalogAdaptive";
    case "catalog_capability": return "models.reasoningSourceCatalogCapability";
    case "configured": return "models.reasoningSourceConfigured";
    case "protocol_suggestion": return "models.reasoningSourceSuggested";
  }
}

function mappingSourceLabelKey(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  level: ConfigurableReasoningLevel,
  existingUpstream?: UpstreamModel,
  outputTokenLimit?: number | null,
): "models.reasoningMappingCatalog" | "models.reasoningMappingConfigured" | "models.reasoningMappingSuggested" {
  switch (reasoningMappingSource(model, protocol, level, existingUpstream, outputTokenLimit)) {
    case "catalog": return "models.reasoningMappingCatalog";
    case "configured": return "models.reasoningMappingConfigured";
    case "protocol_suggestion": return "models.reasoningMappingSuggested";
  }
}

export function openReasoningModal(model: ProviderCatalogModel, context: ReasoningModalContext): void {
  activeReasoningModel = model;
  activeContext = context;
  const draftReasoningLevels = new Set(sortReasoningLevels(context.currentLevels));

  activeLabels = [];
  activeTestButtons = [];
  activeReadOnlyNote = null;
  activeBudgetLabels = [];
  activeSourceLabels = [];

  const supportedLevels = catalogReasoningLevelsForModel(
    model,
    context.providerProtocol,
    context.existingUpstream,
    context.outputTokenLimit,
  );
  
  const body = document.createElement("div");
  body.className = "reasoning-modal-levels";

  const sourceNote = document.createElement("p");
  sourceNote.className = "reasoning-source-note";
  const sourceLabelKey = configurationSourceLabelKey(model, context.existingUpstream);
  sourceNote.dataset.reasoningSourceLabel = sourceLabelKey;
  sourceNote.textContent = t(sourceLabelKey);
  activeSourceLabels.push(sourceNote);
  body.append(sourceNote);

  for (const level of supportedLevels) {
    const row = document.createElement("div");
    row.className = "reasoning-modal-level-row";

    const label = document.createElement("label");
    label.className = "check-label";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = draftReasoningLevels.has(level);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) draftReasoningLevels.add(level);
      else draftReasoningLevels.delete(level);
    });
    const text = document.createElement("span");
    text.textContent = reasoningLevelLabel(level);
    activeLabels.push(text);
    const sourceBadge = document.createElement("span");
    sourceBadge.className = "reasoning-mapping-source";
    const mappingLabelKey = mappingSourceLabelKey(
      model,
      context.providerProtocol,
      level,
      context.existingUpstream,
      context.outputTokenLimit,
    );
    sourceBadge.dataset.reasoningSourceLabel = mappingLabelKey;
    sourceBadge.textContent = t(mappingLabelKey);
    activeSourceLabels.push(sourceBadge);
    label.append(checkbox, text, sourceBadge);

    const testArea = document.createElement("div");
    testArea.className = "reasoning-level-test-area";
    const result = document.createElement("span");
    result.className = "reasoning-level-test-result";
    result.setAttribute("role", "status");

    const testBtn = document.createElement("button");
    testBtn.type = "button";
    testBtn.className = "secondary compact-button";
    testBtn.textContent = t("models.testConnection");
    activeTestButtons.push(testBtn);
    
    testBtn.addEventListener("click", () => {
      void context.runBusy(testBtn, async () => {
        result.className = "reasoning-level-test-result pending";
        result.textContent = t("models.testing");
        const response = await context.testProviderModelConnection(
          context.providerFromForm(),
          model.id,
          level,
          null,
          resolveReasoningMappingForModel(
            model,
            context.providerProtocol,
            level,
            context.existingUpstream,
            context.outputTokenLimit,
          ).mapping,
        );
        if (response.success) {
          result.className = "reasoning-level-test-result success";
          result.textContent = t("models.testSuccess", { time: response.durationMs });
          result.title = result.textContent;
        } else {
          const message = connectionTestErrorMessage(response);
          result.className = "reasoning-level-test-result error";
          result.textContent = t("models.testFailed", { msg: message });
          result.title = message;
        }
      }, t("models.testing"));
    });

    testArea.append(result, testBtn);
    row.append(label, testArea);
    body.append(row);
  }

  currentModal = createModal({
    title: `${t("models.reasoningConfig")} · ${model.displayName}`,
    subtitle: t("models.reasoningSubtitle"),
    body,
    dialogClassName: "reasoning-modal-dialog",
    okLabel: t("models.confirm"),
    cancelLabel: t("models.cancel"),
    onOk: () => {
      context.onConfirm(
        model.id,
        new Set(sortReasoningLevels(draftReasoningLevels)),
        {
          thinkingBudget: context.currentThinkingBudgets.thinkingBudget,
          minThinkingBudget: context.currentThinkingBudgets.minThinkingBudget
        },
      );
      currentModal?.close();
    },
    onClosed: () => {
      activeReasoningModel = null;
      activeContext = null;
      currentModal = null;
      activeLabels = [];
      activeTestButtons = [];
      activeReadOnlyNote = null;
      activeBudgetLabels = [];
      activeSourceLabels = [];
    },
  });

  // Focus first checkbox
  window.setTimeout(() => {
    const firstLevel = body.querySelector<HTMLInputElement>('input:not([disabled])');
    if (firstLevel) firstLevel.focus();
  }, 0);
}
