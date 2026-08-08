import { element, visibleFocusableElements } from "../utils/domUtils";
import type { ProviderCatalogModel } from "../types/catalog";
import type { Provider, ProviderProtocol, UpstreamModel } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningMapping } from "../types/reasoning";
import {
  catalogReasoningLevelsForModel,
  catalogReasoningMappingsForModel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../utils/reasoningUtils";
import { t, subscribeLanguage } from "../i18n";
import { connectionTestErrorMessage } from "../utils/connectionTestUtils";

interface ReasoningModalContext {
  providerProtocol: ProviderProtocol;
  existingUpstream?: UpstreamModel;
  currentLevels: ReadonlySet<ConfigurableReasoningLevel>;
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
  onConfirm: (modelId: string, levels: Set<ConfigurableReasoningLevel>) => void;
}

let activeReasoningModel: ProviderCatalogModel | null = null;
let draftReasoningLevels = new Set<ConfigurableReasoningLevel>();
let activeContext: ReasoningModalContext | null = null;
let reasoningReturnFocus: HTMLElement | null = null;

subscribeLanguage(() => {
  if (!activeReasoningModel || !activeContext) return;
  element<HTMLElement>("#reasoning-modal-title").textContent =
    `${t("models.reasoningConfig")} · ${activeReasoningModel.displayName}`;
  const supportedLevels = catalogReasoningLevelsForModel(
    activeReasoningModel,
    activeContext.providerProtocol,
    activeContext.existingUpstream,
  );
  document.querySelectorAll<HTMLSpanElement>("#reasoning-modal-levels .check-label > span").forEach((label, index) => {
    const level = supportedLevels[index];
    if (level) label.textContent = reasoningLevelLabel(level);
  });
  document.querySelectorAll<HTMLButtonElement>("#reasoning-modal-levels button").forEach((button) => {
    button.textContent = t("models.testConnection");
  });
  const readOnlyNote = document.querySelector<HTMLElement>("#reasoning-modal-levels .reasoning-modal-readonly-note");
  if (readOnlyNote) readOnlyNote.textContent = t("models.reasoningLevelsReadOnly");
});

export function openReasoningModal(model: ProviderCatalogModel, context: ReasoningModalContext): void {
  reasoningReturnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  activeReasoningModel = model;
  activeContext = context;
  draftReasoningLevels = new Set(sortReasoningLevels(context.currentLevels));

  const reasoningModalTitle = element<HTMLElement>("#reasoning-modal-title");
  const reasoningModalLevelsContainer = element<HTMLDivElement>("#reasoning-modal-levels");
  reasoningModalTitle.textContent = `${t("models.reasoningConfig")} · ${model.displayName}`;
  reasoningModalLevelsContainer.replaceChildren();

  const supportedLevels = catalogReasoningLevelsForModel(
    model,
    context.providerProtocol,
    context.existingUpstream,
  );
  const hasCatalogReasoningLevels =
    (model.reasoning?.levels ?? []).some((level) => level !== "off" && level !== "auto")
    || Object.keys(model.reasoning?.mappings ?? {}).some((level) => level !== "off" && level !== "auto");
  if (hasCatalogReasoningLevels) {
    const note = document.createElement("p");
    note.className = "reasoning-modal-readonly-note";
    note.textContent = t("models.reasoningLevelsReadOnly");
    reasoningModalLevelsContainer.append(note);
  }
  for (const level of supportedLevels) {
    const row = document.createElement("div");
    row.className = "reasoning-modal-level-row";

    const label = document.createElement("label");
    label.className = "check-label";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = draftReasoningLevels.has(level);
    // 上游返回的是可用选项，勾选状态由用户决定。
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) draftReasoningLevels.add(level);
      else draftReasoningLevels.delete(level);
    });
    const text = document.createElement("span");
    text.textContent = reasoningLevelLabel(level);
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
    testBtn.addEventListener("click", () => {
      const currentContext = activeContext;
      if (!currentContext) return;
      void currentContext.runBusy(testBtn, async () => {
        result.className = "reasoning-level-test-result pending";
        result.textContent = t("models.testing");
        const response = await currentContext.testProviderModelConnection(
          currentContext.providerFromForm(),
          model.id,
          level,
          null,
          model.reasoning?.mappings?.[level]
            ?? currentContext.existingUpstream?.capabilities.reasoning.levels[level]
            ?? catalogReasoningMappingsForModel(model, currentContext.providerProtocol)[level]
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
    reasoningModalLevelsContainer.append(row);
  }

  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  reasoningModal.hidden = false;
  window.setTimeout(() => {
    const firstLevel = reasoningModalLevelsContainer.querySelector<HTMLInputElement>(
      'input:not([disabled])',
    );
    (firstLevel ?? element<HTMLButtonElement>("#confirm-reasoning-modal")).focus();
  }, 0);
}

export function closeReasoningModal(): void {
  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  reasoningModal.hidden = true;
  activeReasoningModel = null;
  activeContext = null;
  const returnFocus = reasoningReturnFocus;
  reasoningReturnFocus = null;
  if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
}

export function setupReasoningModal(): void {
  const confirmReasoningModalButton = element<HTMLButtonElement>("#confirm-reasoning-modal");
  const cancelReasoningModalButton = element<HTMLButtonElement>("#cancel-reasoning-modal");
  const closeReasoningModalButton = element<HTMLButtonElement>("#close-reasoning-modal");
  const reasoningModalBackdrop = element<HTMLDivElement>("#reasoning-modal-backdrop");
  const confirmModal = element<HTMLDivElement>("#confirm-modal");
  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");

  reasoningModal.hidden = true;

  confirmReasoningModalButton.addEventListener("click", () => {
    if (!activeReasoningModel || !activeContext) return;
    const modelId = activeReasoningModel.id;
    activeContext.onConfirm(modelId, new Set(sortReasoningLevels(draftReasoningLevels)));
    closeReasoningModal();
  });

  cancelReasoningModalButton.addEventListener("click", closeReasoningModal);
  closeReasoningModalButton.addEventListener("click", closeReasoningModal);
  reasoningModalBackdrop.addEventListener("click", closeReasoningModal);
  document.addEventListener("keydown", (event) => {
    if (reasoningModal.hidden || !confirmModal.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closeReasoningModal();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = visibleFocusableElements(reasoningModal);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !reasoningModal.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !reasoningModal.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  });
}
