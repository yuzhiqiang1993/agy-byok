import { t, subscribeLanguage, isTranslationKey, type TranslationKey } from "../i18n";
import type { ProviderCatalogModel } from "../types/catalog";
import type { Provider, ProviderProtocol, UpstreamModel } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type {
  ConfigurableReasoningLevel,
  ReasoningMapping,
  ThinkingBudgetConfig,
} from "../types/reasoning";
import {
  catalogReasoningLevelsForModel,
  catalogReasoningMappingsForModel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../utils/reasoningUtils";
import { connectionTestErrorMessage } from "../utils/connectionTestUtils";
import { createModal, type ModalInstance } from "./common/Modal";

interface ReasoningModalContext {
  providerProtocol: ProviderProtocol;
  existingUpstream?: UpstreamModel;
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

subscribeLanguage(() => {
  if (!activeReasoningModel || !activeContext || !currentModal) return;
  
  const supportedLevels = catalogReasoningLevelsForModel(
    activeReasoningModel,
    activeContext.providerProtocol,
    activeContext.existingUpstream,
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
});

function createBudgetField(
  labelKey: TranslationKey,
  value: number | null,
  minimum: number,
): { field: HTMLLabelElement; input: HTMLInputElement; label: HTMLSpanElement } {
  const field = document.createElement("label");
  field.className = "catalog-token-field";
  const label = document.createElement("span");
  label.dataset.reasoningBudgetLabel = labelKey;
  label.textContent = t(labelKey);
  const input = document.createElement("input");
  input.type = "number";
  input.className = "catalog-token-input";
  input.min = String(minimum);
  input.step = "1";
  input.value = value == null ? "" : String(value);
  input.addEventListener("input", () => input.setCustomValidity(""));
  field.append(label, input);
  return { field, input, label };
}

function readBudgetInput(
  input: HTMLInputElement,
  minimum: number,
  invalidMessage: string,
): number | null | undefined {
  const raw = input.value.trim();
  if (!raw) return null;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value < minimum) {
    input.setCustomValidity(invalidMessage);
    input.reportValidity();
    return undefined;
  }
  input.setCustomValidity("");
  return value;
}

export function openReasoningModal(model: ProviderCatalogModel, context: ReasoningModalContext): void {
  activeReasoningModel = model;
  activeContext = context;
  const draftReasoningLevels = new Set(sortReasoningLevels(context.currentLevels));

  activeLabels = [];
  activeTestButtons = [];
  activeReadOnlyNote = null;
  activeBudgetLabels = [];

  const supportedLevels = catalogReasoningLevelsForModel(
    model,
    context.providerProtocol,
    context.existingUpstream,
  );
  
  const body = document.createElement("div");
  body.className = "reasoning-modal-levels";

  const budgetEditable = context.providerProtocol === "gemini_generate_content";
  let defaultBudget: ReturnType<typeof createBudgetField> | null = null;
  let minimumBudget: ReturnType<typeof createBudgetField> | null = null;

  if (budgetEditable) {
    const budgetFields = document.createElement("div");
    budgetFields.className = "catalog-token-fields";
    defaultBudget = createBudgetField(
      "models.thinkingBudget",
      context.currentThinkingBudgets.thinkingBudget,
      -1,
    );
    minimumBudget = createBudgetField(
      "models.minThinkingBudget",
      context.currentThinkingBudgets.minThinkingBudget,
      1,
    );
    defaultBudget.input.title = t("models.thinkingBudgetHint");
    minimumBudget.input.title = t("models.minThinkingBudgetHint");
    
    activeBudgetLabels.push(defaultBudget.label, minimumBudget.label);
    budgetFields.append(defaultBudget.field, minimumBudget.field);
    body.append(budgetFields);
  }
  
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
    label.append(checkbox, text);

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
          model.reasoning?.mappings?.[level]
            ?? context.existingUpstream?.capabilities.reasoning.levels[level]
            ?? catalogReasoningMappingsForModel(model, context.providerProtocol)[level]
            ?? null,
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
      let thinkingBudget: number | null | undefined = null;
      let minThinkingBudget: number | null | undefined = null;

      if (budgetEditable && defaultBudget && minimumBudget) {
        thinkingBudget = readBudgetInput(
          defaultBudget.input,
          -1,
          t("models.invalidThinkingBudget"),
        );
        if (thinkingBudget === undefined) return;
        
        minThinkingBudget = readBudgetInput(
          minimumBudget.input,
          1,
          t("models.invalidMinThinkingBudget"),
        );
        if (minThinkingBudget === undefined) return;
        
        if (minThinkingBudget != null && (
          thinkingBudget === 0
          || (thinkingBudget != null && thinkingBudget > 0 && minThinkingBudget > thinkingBudget)
        )) {
          minimumBudget.input.setCustomValidity(t("models.minThinkingBudgetExceedsDefault"));
          minimumBudget.input.reportValidity();
          return;
        }
      } else {
        thinkingBudget = context.currentThinkingBudgets.thinkingBudget;
        minThinkingBudget = context.currentThinkingBudgets.minThinkingBudget;
      }
      
      context.onConfirm(
        model.id,
        new Set(sortReasoningLevels(draftReasoningLevels)),
        { thinkingBudget, minThinkingBudget },
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
    },
  });

  // Focus first checkbox
  window.setTimeout(() => {
    const firstLevel = body.querySelector<HTMLInputElement>('input:not([disabled])');
    if (firstLevel) firstLevel.focus();
  }, 0);
}
