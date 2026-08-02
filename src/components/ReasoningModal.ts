import { element } from "../utils/domUtils";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ConfigurableReasoningLevel } from "../types/reasoning";
import {
  testProviderModelConnection,
  setProviderEditorDirty,
  renderCatalogModels,
  editingProviderId,
  catalogReasoningLevelsByModel,
  catalogReasoningEnabledModelIds,
  changedCatalogReasoningModelIds,
  selectedProtocol,
  withProviderEditorBusy,
  providerFromForm,
} from "./ProviderEditor";
import { reasoningLevelLabel, sortReasoningLevels, catalogReasoningLevelsForModel } from "../utils/reasoningUtils";
import { store } from "../store/appStore";

export let activeReasoningModel: ProviderCatalogModel | null = null;
export let draftReasoningLevels = new Set<ConfigurableReasoningLevel>();

export function openReasoningModal(model: ProviderCatalogModel): void {
  activeReasoningModel = model;

  const existingUpstream = editingProviderId
    ? store.config?.upstream_models.find(
        (item) => item.provider_id === editingProviderId && item.upstream_model_id === model.id,
      )
    : undefined;
  const currentLevels = catalogReasoningLevelsByModel.get(model.id) ?? new Set<ConfigurableReasoningLevel>();
  draftReasoningLevels = new Set(sortReasoningLevels(currentLevels));

  const reasoningModalTitle = element<HTMLElement>("#reasoning-modal-title");
  const reasoningModalLevelsContainer = element<HTMLDivElement>("#reasoning-modal-levels");

  reasoningModalTitle.textContent = `推理强度配置 · ${model.displayName}`;
  reasoningModalLevelsContainer.replaceChildren();

  const supportedLevels = catalogReasoningLevelsForModel(model, selectedProtocol(), existingUpstream);
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
    label.append(checkbox, text);

    const testArea = document.createElement("div");
    testArea.className = "reasoning-level-test-area";
    const result = document.createElement("span");
    result.className = "reasoning-level-test-result";
    result.setAttribute("role", "status");

    const testBtn = document.createElement("button");
    testBtn.type = "button";
    testBtn.className = "secondary compact-button";
    testBtn.textContent = "测试";
    testBtn.addEventListener("click", () => {
      void withProviderEditorBusy(testBtn, async () => {
        result.className = "reasoning-level-test-result pending";
        result.textContent = "测试中…";
        const response = await testProviderModelConnection(providerFromForm(), model.id, level, null);
        if (response.success) {
          result.className = "reasoning-level-test-result success";
          result.textContent = `通过 · ${response.durationMs} ms`;
        } else {
          result.className = "reasoning-level-test-result error";
          result.textContent = `失败 · ${response.message}`;
        }
        result.title = response.message ?? "";
      }, "测试中…");
    });

    testArea.append(result, testBtn);
    row.append(label, testArea);
    reasoningModalLevelsContainer.append(row);
  }

  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  reasoningModal.hidden = false;
}

export function closeReasoningModal(): void {
  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  reasoningModal.hidden = true;
  activeReasoningModel = null;
}

export function setupReasoningModal(): void {
  const confirmReasoningModalButton = element<HTMLButtonElement>("#confirm-reasoning-modal");
  const cancelReasoningModalButton = element<HTMLButtonElement>("#cancel-reasoning-modal");
  const closeReasoningModalButton = element<HTMLButtonElement>("#close-reasoning-modal");
  const reasoningModalBackdrop = element<HTMLDivElement>("#reasoning-modal-backdrop");

  element<HTMLDivElement>("#reasoning-modal").hidden = true;

  confirmReasoningModalButton.addEventListener("click", () => {
    if (!activeReasoningModel) return;
    const modelId = activeReasoningModel.id;
    if (draftReasoningLevels.size > 0) {
      catalogReasoningEnabledModelIds.add(modelId);
      catalogReasoningLevelsByModel.set(modelId, new Set(sortReasoningLevels(draftReasoningLevels)));
    } else {
      catalogReasoningEnabledModelIds.delete(modelId);
      catalogReasoningLevelsByModel.delete(modelId);
    }
    changedCatalogReasoningModelIds.add(modelId);
    setProviderEditorDirty(true);
    renderCatalogModels();
    closeReasoningModal();
  });

  cancelReasoningModalButton.addEventListener("click", closeReasoningModal);
  closeReasoningModalButton.addEventListener("click", closeReasoningModal);
  reasoningModalBackdrop.addEventListener("click", closeReasoningModal);
}
