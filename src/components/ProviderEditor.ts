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
import { store } from "../store/appStore";
import { setupProviderEditorBindings, switchToStep } from "./providerEditor/ProviderEditorBindings";

// Provider 与单模型共用一套弹窗，模式只控制展示范围与交互文案。
type ProviderEditorMode =
  | { kind: "provider" }
  | {
      kind: "model";
      upstreamId: string;
      catalogModelId: string;
      displayName: string;
    };

let providerEditorBusy = false;
let providerEditorReturnFocus: HTMLElement | null = null;
let providerEditorReturnModelKey: { providerId: string; upstreamId: string } | null = null;
let providerEditorMode: ProviderEditorMode = { kind: "provider" };

function focusedCatalogModelId(): string | null {
  return providerEditorMode.kind === "model" ? providerEditorMode.catalogModelId : null;
}

function refreshProviderEditorHeader(): void {
  const title = element<HTMLElement>("#provider-form-title");
  const kicker = element<HTMLElement>("#provider-form-kicker");
  if (providerEditorMode.kind === "model") {
    title.textContent = t("models.editModelFor", { name: providerEditorMode.displayName });
    kicker.textContent = t("models.editModel");
    return;
  }
  const provider = providerForm.editingProviderId
    ? store.config.providers.find((item) => item.id === providerForm.editingProviderId)
    : undefined;
  title.textContent = provider
    ? `${t("models.editProviderTitle")} · ${provider.name}`
    : t("models.addProviderTitle");
  kicker.textContent = t(provider ? "models.editKicker" : "models.addKicker");
}

function syncProviderEditorModeUI(): void {
  const modelMode = providerEditorMode.kind === "model";
  const panel = element<HTMLElement>("#provider-form-panel");
  panel.dataset.editorMode = modelMode ? "model" : "provider";
  element<HTMLElement>("#wizard-stepper").hidden = modelMode;
  element<HTMLElement>(".catalog-toolbar").hidden = modelMode;
  element<HTMLButtonElement>("#back-to-config").hidden = modelMode;
  element<HTMLElement>("#selected-model-count").hidden = modelMode;
  element<HTMLElement>("#catalog-title").textContent = modelMode
    ? t("models.modelConfigTitle")
    : t("models.catalogTitle");
  refreshProviderEditorHeader();
}

function invalidatePendingProviderSave(): void {
  providerSave.invalidatePendingProviderSave();
}

function refreshProviderEditorControls(): void {
  const hasSelection = providerCatalog.getProviderCatalogState().selectedCatalogModelIds.size > 0;
  const saveProviderButton = element<HTMLButtonElement>("#save-provider");
  const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
  saveProviderButton.disabled = providerEditorBusy || !getProviderEditorDirty() || !hasSelection;
  cancelProviderButton.disabled = providerEditorBusy;
  if (!providerEditorBusy) {
    const saveKey = providerEditorMode.kind === "model"
      ? "models.saveModel" as const
      : "models.saveProvider" as const;
    saveProviderButton.dataset.i18n = saveKey;
    saveProviderButton.textContent = t(saveKey);
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
  const message = providerEditorMode.kind === "model"
    ? t("models.discardModelChanges")
    : t("models.discardChanges");
  try {
    return await confirm(message, { kind: "warning" });
  } catch (error) {
    console.error("Native confirm dialog failed:", error);
    return window.confirm(message);
  }
}

function createProviderCatalogContext(): providerCatalog.ProviderCatalogContext {
  return {
    getEditingProviderId: () => providerForm.editingProviderId,
    getFocusedCatalogModelId: focusedCatalogModelId,
    selectedProtocol: providerForm.selectedProtocol,
    providerFromForm: providerForm.providerFromForm,
    setProviderEditorDirty,
    withProviderEditorBusy,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
  };
}

function resetProviderEditor(): void {
  providerEditorMode = { kind: "provider" };
  providerForm.resetProviderForm({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
    onPresetSelected: () => switchToStep("config"),
  });
  syncProviderEditorModeUI();
}

export async function closeProviderEditor(force = false): Promise<boolean> {
  if (!force && !(await confirmDiscardProviderChanges())) return false;
  const returnFocus = providerEditorReturnFocus;
  const returnModelKey = providerEditorReturnModelKey;
  providerEditorReturnFocus = null;
  providerEditorReturnModelKey = null;
  element<HTMLElement>("#provider-form-panel").hidden = true;
  document.body.classList.remove("modal-open");
  resetProviderEditor();
  window.setTimeout(() => {
    if (returnFocus?.isConnected) {
      returnFocus.focus();
      return;
    }
    if (!returnModelKey) return;
    const replacement = [...document.querySelectorAll<HTMLButtonElement>(".model-edit-btn")]
      .find((button) => button.dataset.providerId === returnModelKey.providerId
        && button.dataset.upstreamId === returnModelKey.upstreamId);
    replacement?.focus();
  }, 0);
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
  providerEditorReturnModelKey = null;
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

export async function openProviderModelEditor(
  providerId: string,
  upstreamId: string,
): Promise<void> {
  const provider = store.config.providers.find((item) => item.id === providerId);
  const upstream = store.config.upstream_models.find(
    (item) => item.id === upstreamId && item.provider_id === providerId,
  );
  const providerModelIds = new Set<string>();
  const hasDuplicateModelId = store.config.upstream_models.some((item) => {
    if (item.provider_id !== providerId) return false;
    if (providerModelIds.has(item.upstream_model_id)) return true;
    providerModelIds.add(item.upstream_model_id);
    return false;
  });
  if (!provider || !upstream || hasDuplicateModelId) {
    showNotice(t("models.modelEditUnavailable"), "error");
    return;
  }

  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  if (!providerFormPanel.hidden
    && providerEditorMode.kind === "model"
    && providerEditorMode.upstreamId === upstreamId) {
    providerFormPanel.querySelector<HTMLElement>(
      "#catalog-model-list select:not([disabled]), #catalog-model-list input:not([disabled]), #catalog-model-list button:not([disabled])",
    )?.focus();
    return;
  }
  if (!(await confirmDiscardProviderChanges())) return;

  providerEditorReturnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  providerEditorReturnModelKey = { providerId, upstreamId };
  resetProviderEditor();
  providerForm.beginProviderEdit(providerId);
  providerEditorMode = {
    kind: "model",
    upstreamId,
    catalogModelId: upstream.upstream_model_id,
    displayName: upstream.display_name,
  };
  syncProviderEditorModeUI();
  providerFormPanel.hidden = false;
  document.body.classList.add("modal-open");
  element<HTMLButtonElement>("#step-node-preset").disabled = true;
  providerCatalog.loadConfiguredProviderCatalog(createProviderCatalogContext(), providerId);
  switchToStep("catalog");
  refreshProviderEditorControls();

  window.setTimeout(() => {
    providerFormPanel.querySelector<HTMLElement>(
      "#catalog-model-list select:not([disabled]), #catalog-model-list input:not([disabled]), #catalog-model-list button:not([disabled])",
    )?.focus();
  }, 100);
}

async function fetchProviderCatalog(): Promise<void> {
  await providerCatalog.fetchProviderCatalog(createProviderCatalogContext());
}

function renderCatalogModels(): void {
  providerCatalog.renderCatalogModels(createProviderCatalogContext());
}

function createProviderSaveContext(): providerSave.ProviderSaveContext {
  const editedModelName = providerEditorMode.kind === "model"
    ? providerEditorMode.displayName
    : null;
  return {
    providerFromForm: providerForm.providerFromForm,
    getEditingProviderId: () => providerForm.editingProviderId,
    getCatalogState: providerCatalog.getProviderCatalogState,
    setProviderEditorDirty,
    refreshProviderEditorControls,
    closeProviderEditor,
    savedMessage: editedModelName
      ? () => t("models.modelSaved", { name: editedModelName })
      : undefined,
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
    refreshHeader: syncProviderEditorModeUI,
  });
  providerForm.setupProviderPresets({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty,
    invalidatePendingProviderSave,
    refreshProviderEditorControls,
    onPresetSelected: () => switchToStep("config"),
  });
}
