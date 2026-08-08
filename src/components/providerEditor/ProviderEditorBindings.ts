import { subscribeLanguage, t } from "../../i18n";
import { store } from "../../store/appStore";
import { element, visibleFocusableElements } from "../../utils/domUtils";
import * as providerCatalog from "../../features/providers/providerCatalog";
import * as providerForm from "../../features/providers/providerForm";
import { isProviderEditorDirty } from "../../features/providers/providerState";

interface ProviderEditorBindings {
  setDirty: (dirty: boolean) => void;
  withBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
  fetchCatalog: () => Promise<void>;
  saveProvider: () => Promise<void>;
  renderCatalogModels: () => void;
  closeEditor: () => Promise<boolean>;
  openEditor: () => Promise<void>;
  refreshControls: () => void;
  createCatalogContext: () => providerCatalog.ProviderCatalogContext;
}

function bindFormEvents(bindings: ProviderEditorBindings): void {
  const form = element<HTMLFormElement>("#provider-form");
  const saveButton = element<HTMLButtonElement>("#save-provider");
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void bindings.withBusy(saveButton, bindings.saveProvider, t("models.saving"));
  });
  element<HTMLButtonElement>("#fetch-provider-models").addEventListener("click", (event) => {
    void bindings.withBusy(
      event.currentTarget as HTMLButtonElement,
      bindings.fetchCatalog,
      t("models.fetching"),
    );
  });
  element<HTMLInputElement>("#provider-name").addEventListener("input", () => bindings.setDirty(true));
  element<HTMLInputElement>("#provider-base-url").addEventListener("input", () => {
    providerForm.updateSuggestedEndpoints(providerCatalog.resetCatalogResults);
    bindings.setDirty(true);
  });
  element<HTMLSelectElement>("#protocol").addEventListener("change", () => {
    providerForm.updateSuggestedEndpoints(providerCatalog.resetCatalogResults);
    bindings.setDirty(true);
  });
  for (const selector of ["#models-endpoint", "#generate-endpoint", "#api-key"]) {
    element<HTMLInputElement>(selector).addEventListener("input", () => {
      providerCatalog.resetCatalogResults();
      bindings.setDirty(true);
    });
  }
  element<HTMLButtonElement>("#toggle-api-key").addEventListener("click", () => {
    const input = element<HTMLInputElement>("#api-key");
    input.type = input.type === "text" ? "password" : "text";
    providerForm.syncApiKeyToggle();
  });
}

function bindCatalogEvents(bindings: ProviderEditorBindings): void {
  element<HTMLButtonElement>("#back-to-config").addEventListener("click", () => {
    element<HTMLElement>("#catalog-results").classList.remove("active");
    element<HTMLElement>("#catalog-results").hidden = true;
    element<HTMLElement>("#provider-step-config").hidden = false;
    element<HTMLElement>("#provider-step-config").classList.add("active");
  });
  element<HTMLInputElement>("#catalog-search").addEventListener("input", bindings.renderCatalogModels);
  element<HTMLInputElement>("#select-all-models").addEventListener("change", (event) => {
    const selected = (event.currentTarget as HTMLInputElement).checked;
    const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
    const visibleIds = providerCatalog.getProviderCatalogState().catalogModels
      .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
      .map((model) => model.id);
    providerCatalog.setCatalogModelSelection(visibleIds, selected);
    bindings.setDirty(true);
    bindings.renderCatalogModels();
  });
}

function bindModalEvents(bindings: ProviderEditorBindings): void {
  for (const selector of ["#close-provider-modal", "#provider-modal-backdrop", "#cancel-provider"]) {
    element<HTMLElement>(selector).addEventListener("click", () => void bindings.closeEditor());
  }
  element<HTMLButtonElement>("#open-provider-form").addEventListener(
    "click",
    () => void bindings.openEditor(),
  );
  window.addEventListener("beforeunload", (event) => {
    if (!isProviderEditorDirty()) return;
    event.preventDefault();
    event.returnValue = "";
  });
}

function bindKeyboardNavigation(bindings: ProviderEditorBindings): void {
  const confirmModal = element<HTMLDivElement>("#confirm-modal");
  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  const panel = element<HTMLElement>("#provider-form-panel");
  document.addEventListener("keydown", (event) => {
    if (!confirmModal.hidden || !reasoningModal.hidden) return;
    if (panel.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void bindings.closeEditor();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = visibleFocusableElements(panel);
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
}

function bindLanguageRefresh(bindings: ProviderEditorBindings): void {
  subscribeLanguage(() => {
    providerForm.updateProtocolHelp();
    providerForm.syncApiKeyToggle();
    providerCatalog.renderCatalogStatus();
    if (!element<HTMLElement>("#catalog-results").hidden
      && providerCatalog.getProviderCatalogState().catalogModels.length > 0) {
      bindings.renderCatalogModels();
    } else {
      providerCatalog.updateCatalogSelection(bindings.createCatalogContext());
    }
    bindings.refreshControls();
    if (element<HTMLElement>("#provider-form-panel").hidden) return;
    if (providerForm.editingProviderId) {
      const provider = store.config.providers.find((item) => item.id === providerForm.editingProviderId);
      if (provider) {
        element<HTMLElement>("#provider-form-title").textContent = `${t("models.editProviderTitle")} · ${provider.name}`;
      }
    } else {
      element<HTMLElement>("#provider-form-title").textContent = t("models.addProviderTitle");
    }
  });
}

export function setupProviderEditorBindings(bindings: ProviderEditorBindings): void {
  const catalogContext = bindings.createCatalogContext();
  providerForm.setupProviderPresets({
    resetCatalogResults: providerCatalog.resetCatalogResults,
    setProviderEditorDirty: bindings.setDirty,
    invalidatePendingProviderSave: catalogContext.invalidatePendingProviderSave,
    refreshProviderEditorControls: bindings.refreshControls,
  });
  bindFormEvents(bindings);
  bindCatalogEvents(bindings);
  bindModalEvents(bindings);
  bindKeyboardNavigation(bindings);
  bindLanguageRefresh(bindings);
  providerForm.syncApiKeyToggle();
}
