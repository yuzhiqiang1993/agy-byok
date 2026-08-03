import { confirm } from "@tauri-apps/plugin-dialog";
import type { Provider, ProviderProtocol } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ReasoningLevel } from "../types/reasoning";
import { store } from "../store/appStore";
import {
  isProviderEditorDirty as getProviderEditorDirty,
  setProviderEditorDirtyState,
} from "../features/providers/providerState";
import { element, withBusy } from "../utils/domUtils";
import { showNotice } from "./NoticeBar";
import * as providerCatalog from "../features/providers/providerCatalog";
import * as providerForm from "../features/providers/providerForm";
import { testProviderModelConnection as testProviderModelConnectionImpl } from "../features/providers/providerTesting";
import * as providerSave from "../features/providers/providerSave";
import { closeReasoningModal } from "./ReasoningModal";
import { t, subscribeLanguage } from "../i18n";

export {
  editingProviderId,
  draftProviderId,
} from "../features/providers/providerForm";
export {
  catalogModels,
  selectedCatalogModelIds,
  catalogReasoningLevelsByModel,
  catalogCustomReasoningByModel,
  catalogVisionEnabledModelIds,
  catalogToolsEnabledModelIds,
  catalogReasoningEnabledModelIds,
  changedCatalogCapabilityModelIds,
  changedCatalogReasoningModelIds,
  legacyCatalogModelIds,
} from "../features/providers/providerCatalog";

let providerEditorBusy = false;
let providerEditorReturnFocus: HTMLElement | null = null;

export function isProviderEditorDirty(): boolean {
  return getProviderEditorDirty();
}

export function invalidatePendingProviderSave(): void {
  providerSave.invalidatePendingProviderSave();
}

function refreshProviderEditorControls(): void {
  const hasSelection = providerCatalog.selectedCatalogModelIds.size > 0;
  const saveProviderButton = element<HTMLButtonElement>("#save-provider");
  const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
  const pendingProviderSavePlan = providerSave.getPendingProviderSavePlan();
  saveProviderButton.disabled = providerEditorBusy || !getProviderEditorDirty() || !hasSelection;
  cancelProviderButton.disabled = providerEditorBusy;
  if (!providerEditorBusy) {
    saveProviderButton.textContent = pendingProviderSavePlan
      ? t("models.confirmSaveRemoval", { count: pendingProviderSavePlan.summary.removedVirtualModels.length })
      : t("models.saveProvider");
  }
}

export function setProviderEditorDirty(dirty: boolean): void {
  setProviderEditorDirtyState(dirty);
  element<HTMLElement>("#provider-editor-dirty").hidden = !dirty;
  if (dirty) invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

function setProviderEditorBusy(busy: boolean): void {
  providerEditorBusy = busy;
  const providerForm = element<HTMLFormElement>("#provider-form");
  const providerList = element<HTMLDivElement>("#provider-list");
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  providerForm.toggleAttribute("inert", busy);
  providerForm.setAttribute("aria-busy", String(busy));
  providerList.toggleAttribute("inert", busy);
  providerFormPanel.dataset.busy = String(busy);
  refreshProviderEditorControls();
}

export async function withProviderEditorBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = t("models.processing"),
): Promise<void> {
  if (providerEditorBusy) return;
  setProviderEditorBusy(true);
  try {
    await withBusy(button, action, busyLabel);
  } finally {
    setProviderEditorBusy(false);
  }
}

export async function confirmDiscardProviderChanges(): Promise<boolean> {
  if (providerEditorBusy) {
    showNotice(t("models.editorBusy"), "error");
    return false;
  }
  if (!getProviderEditorDirty()) return true;
  try {
    return await confirm(t("models.discardChanges"), { kind: "warning" });
  } catch (error) {
    console.error("Native confirm dialog failed:", error);
    return window.confirm(t("models.discardChanges"));
  }
}

export function selectedProtocol(): ProviderProtocol {
  return providerForm.selectedProtocol();
}

export function providerFromForm(): Provider {
  return providerForm.providerFromForm();
}

function createProviderCatalogContext(): providerCatalog.ProviderCatalogContext {
  return {
    getEditingProviderId: () => providerForm.editingProviderId,
    selectedProtocol: providerForm.selectedProtocol,
    providerFromForm: providerForm.providerFromForm,
    setProviderEditorDirty,
    withProviderEditorBusy,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
  };
}

function resetProviderEditor(): void {
  providerForm.resetProviderForm({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
  });
}

export async function closeProviderEditor(force = false): Promise<boolean> {
  if (!force && !(await confirmDiscardProviderChanges())) return false;
  const returnFocus = providerEditorReturnFocus;
  providerEditorReturnFocus = null;
  element<HTMLElement>("#provider-form-panel").hidden = true;
  document.body.classList.remove("modal-open");
  resetProviderEditor();
  if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
  return true;
}

export async function openProviderEditor(providerId: string | null = null): Promise<void> {
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  if (!providerFormPanel.hidden && providerForm.editingProviderId === providerId) {
    element<HTMLInputElement>("#provider-name").focus();
    return;
  }
  if (!(await confirmDiscardProviderChanges())) return;
  providerEditorReturnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  resetProviderEditor();
  providerForm.beginProviderEdit(providerId);
  providerFormPanel.hidden = false;
  document.body.classList.add("modal-open");
  window.setTimeout(() => element<HTMLInputElement>("#provider-name").focus(), 100);
}

async function fetchProviderCatalog(): Promise<void> {
  await providerCatalog.fetchProviderCatalog(createProviderCatalogContext());
}

export async function testProviderModelConnection(
  provider: Provider,
  upstreamModelId: string,
  reasoningLevel: ReasoningLevel | null,
  customReasoningValue: string | null,
): Promise<ModelConnectionTestResult> {
  return testProviderModelConnectionImpl(
    provider,
    upstreamModelId,
    reasoningLevel,
    customReasoningValue,
  );
}

export function renderCatalogModels(): void {
  providerCatalog.renderCatalogModels(createProviderCatalogContext());
}

function createProviderSaveContext(): providerSave.ProviderSaveContext {
  return {
    providerFromForm: providerForm.providerFromForm,
    getEditingProviderId: () => providerForm.editingProviderId,
    getCatalogState: providerCatalog.getProviderCatalogState,
    setProviderEditorDirty,
    refreshProviderEditorControls,
    closeProviderEditor,
  };
}

async function saveProvider(): Promise<void> {
  await providerSave.saveProvider(createProviderSaveContext());
}

export function setupProviderEditor(): void {
  providerForm.setupProviderPresets({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
  });
  const providerFormElement = element<HTMLFormElement>("#provider-form");
  const saveProviderButton = element<HTMLButtonElement>("#save-provider");

  providerFormElement.addEventListener("submit", (event) => {
    event.preventDefault();
    void withProviderEditorBusy(saveProviderButton, saveProvider, t("models.saving"));
  });

  element<HTMLButtonElement>("#fetch-provider-models").addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    void withProviderEditorBusy(button, fetchProviderCatalog, t("models.fetching"));
  });

  element<HTMLInputElement>("#provider-name").addEventListener("input", () => {
    setProviderEditorDirty(true);
  });
  element<HTMLInputElement>("#provider-base-url").addEventListener("input", () => {
    providerForm.updateSuggestedEndpoints(providerCatalog.resetCatalogResults);
    setProviderEditorDirty(true);
  });
  element<HTMLSelectElement>("#protocol").addEventListener("change", () => {
    providerForm.updateSuggestedEndpoints(providerCatalog.resetCatalogResults);
    setProviderEditorDirty(true);
  });
  for (const selector of ["#models-endpoint", "#generate-endpoint", "#api-key"]) {
    element<HTMLInputElement>(selector).addEventListener("input", () => {
      providerCatalog.resetCatalogResults();
      setProviderEditorDirty(true);
    });
  }

  element<HTMLInputElement>("#catalog-search").addEventListener("input", renderCatalogModels);
  element<HTMLInputElement>("#select-all-models").addEventListener("change", (event) => {
    const checkbox = event.currentTarget as HTMLInputElement;
    const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
    const visibleIds = providerCatalog.catalogModels
      .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
      .map((model) => model.id);
    for (const id of visibleIds) {
      if (checkbox.checked) providerCatalog.selectedCatalogModelIds.add(id);
      else providerCatalog.selectedCatalogModelIds.delete(id);
    }
    setProviderEditorDirty(true);
    renderCatalogModels();
  });

  element<HTMLButtonElement>("#close-provider-modal").addEventListener("click", () => {
    void closeProviderEditor();
  });

  element<HTMLElement>("#provider-modal-backdrop").addEventListener("click", () => {
    void closeProviderEditor();
  });

  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");

  document.addEventListener("keydown", (event) => {
    if (!reasoningModal.hidden) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeReasoningModal();
        return;
      }
    }
    if (providerFormPanel.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void closeProviderEditor();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = [...providerFormPanel.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), summary, [tabindex]:not([tabindex="-1"])',
    )].filter((item) => !item.hidden && item.getClientRects().length > 0);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });

  window.addEventListener("beforeunload", (event) => {
    if (!getProviderEditorDirty()) return;
    event.preventDefault();
    event.returnValue = "";
  });

  element<HTMLButtonElement>("#toggle-api-key").addEventListener("click", () => {
    const input = element<HTMLInputElement>("#api-key");
    input.type = input.type === "text" ? "password" : "text";
    providerForm.syncApiKeyToggle();
  });

  const openProviderFormButton = element<HTMLButtonElement>("#open-provider-form");
  openProviderFormButton.addEventListener("click", () => openProviderEditor());

  const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
  cancelProviderButton.addEventListener("click", () => {
    void closeProviderEditor();
  });

  providerForm.syncApiKeyToggle();

  subscribeLanguage(() => {
    providerForm.updateProtocolHelp();
    providerForm.syncApiKeyToggle();
    providerCatalog.renderCatalogStatus();
    if (!element<HTMLElement>("#catalog-results").hidden && providerCatalog.catalogModels.length > 0) {
      renderCatalogModels();
    } else {
      providerCatalog.updateCatalogSelection(createProviderCatalogContext());
    }
    refreshProviderEditorControls();
    if (!element<HTMLElement>("#provider-form-panel").hidden) {
      if (providerForm.editingProviderId) {
        const provider = store.config.providers.find((item) => item.id === providerForm.editingProviderId);
        if (provider) {
          element<HTMLElement>("#provider-form-title").textContent = `${t("models.editProviderTitle")} · ${provider.name}`;
        }
      } else {
        element<HTMLElement>("#provider-form-title").textContent = t("models.addProviderTitle");
      }
    }
  });
}
