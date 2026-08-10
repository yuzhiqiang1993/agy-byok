import { confirm } from "@tauri-apps/plugin-dialog";
import {
  isProviderEditorDirty as getProviderEditorDirty,
  setProviderEditorDirtyState,
} from "../features/providers/providerState";
import { element, withBusy } from "../utils/domUtils";
import { showNotice } from "./NoticeBar";
import * as providerCatalog from "../features/providers/providerCatalog";
import * as providerForm from "../features/providers/providerForm";
import * as providerSave from "../features/providers/providerSave";
import { t } from "../i18n";
import { setupProviderEditorBindings, switchToStep } from "./providerEditor/ProviderEditorBindings";

let providerEditorBusy = false;
let providerEditorReturnFocus: HTMLElement | null = null;

function invalidatePendingProviderSave(): void {
  providerSave.invalidatePendingProviderSave();
}

function refreshProviderEditorControls(): void {
  const hasSelection = providerCatalog.getProviderCatalogState().selectedCatalogModelIds.size > 0;
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

function setProviderEditorDirty(dirty: boolean): void {
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

async function withProviderEditorBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = t("models.processing"),
): Promise<void> {
  if (providerEditorBusy) return;
  setProviderEditorBusy(true);
  try {
    await withBusy(button, action, showNotice, busyLabel);
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
    onPresetSelected: () => switchToStep("config"),
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

  if (providerId) {
    element<HTMLButtonElement>("#step-node-preset").disabled = true;
    switchToStep("config");
  } else {
    element<HTMLButtonElement>("#step-node-preset").disabled = false;
    switchToStep("preset");
  }

  window.setTimeout(() => element<HTMLInputElement>("#provider-name").focus(), 100);
}

async function fetchProviderCatalog(): Promise<void> {
  await providerCatalog.fetchProviderCatalog(createProviderCatalogContext());
}

function renderCatalogModels(): void {
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
    notify: showNotice,
  };
}

async function saveProvider(): Promise<void> {
  await providerSave.saveProvider(createProviderSaveContext());
}

export function setupProviderEditor(): void {
  setupProviderEditorBindings({
    setDirty: setProviderEditorDirty,
    withBusy: withProviderEditorBusy,
    fetchCatalog: fetchProviderCatalog,
    saveProvider,
    renderCatalogModels,
    closeEditor: closeProviderEditor,
    openEditor: () => openProviderEditor(),
    refreshControls: refreshProviderEditorControls,
    createCatalogContext: createProviderCatalogContext,
  });
  providerForm.setupProviderPresets({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
    onPresetSelected: () => switchToStep("config"),
  });
}
