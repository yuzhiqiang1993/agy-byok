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

import { showNotice } from "../NoticeBar";

export function switchToStep(step: "preset" | "config" | "catalog"): void {
  const presetStep = element<HTMLElement>("#provider-step-preset");
  const configStep = element<HTMLElement>("#provider-step-config");
  const catalogStep = element<HTMLElement>("#catalog-results");
  const presetNode = element<HTMLButtonElement>("#step-node-preset");
  const configNode = element<HTMLButtonElement>("#step-node-config");
  const catalogNode = element<HTMLButtonElement>("#step-node-catalog");

  if (step === "preset") {
    configStep.classList.remove("active");
    configStep.hidden = true;
    catalogStep.classList.remove("active");
    catalogStep.hidden = true;
    presetStep.hidden = false;
    presetStep.classList.add("active");

    presetNode.classList.add("active");
    configNode.classList.remove("active");
    catalogNode.classList.remove("active");
  } else if (step === "config") {
    presetStep.classList.remove("active");
    presetStep.hidden = true;
    catalogStep.classList.remove("active");
    catalogStep.hidden = true;
    configStep.hidden = false;
    configStep.classList.add("active");

    presetNode.classList.remove("active");
    configNode.classList.add("active");
    configNode.disabled = false;
    catalogNode.classList.remove("active");
  } else {
    presetStep.classList.remove("active");
    presetStep.hidden = true;
    configStep.classList.remove("active");
    configStep.hidden = true;
    catalogStep.hidden = false;
    catalogStep.classList.add("active");

    presetNode.classList.remove("active");
    configNode.classList.remove("active");
    catalogNode.classList.add("active");
    catalogNode.disabled = false;
  }
}

function bindFormEvents(bindings: ProviderEditorBindings): void {
  const form = element<HTMLFormElement>("#provider-form");
  const saveButton = element<HTMLButtonElement>("#save-provider");
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void bindings.withBusy(saveButton, bindings.saveProvider, t("models.saving"));
  });
  element<HTMLButtonElement>("#fetch-provider-models").addEventListener("click", async (event) => {
    await bindings.withBusy(
      event.currentTarget as HTMLButtonElement,
      async () => {
        await bindings.fetchCatalog();
        switchToStep("catalog");
      },
      t("models.fetching"),
    );
  });
  const backToPresetsBtn = document.querySelector<HTMLButtonElement>("#back-to-presets");
  if (backToPresetsBtn) {
    backToPresetsBtn.addEventListener("click", () => {
      switchToStep("preset");
    });
  }
  element<HTMLInputElement>("#provider-name").addEventListener("input", () => {
    bindings.setDirty(true);
  });
  element<HTMLInputElement>("#provider-base-url").addEventListener("input", (event) => {
    const url = (event.currentTarget as HTMLInputElement).value;
    providerForm.updateSuggestedEndpoints(providerCatalog.resetCatalogResults);
    const detected = providerForm.detectPresetFromUrl(url);
    providerForm.syncActivePreset(detected);
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
  const pasteApiKeyBtn = document.querySelector<HTMLButtonElement>("#paste-api-key");
  if (pasteApiKeyBtn) {
    pasteApiKeyBtn.addEventListener("click", async () => {
      try {
        const text = await navigator.clipboard.readText();
        if (text) {
          const input = element<HTMLInputElement>("#api-key");
          input.value = text.trim();
          providerCatalog.resetCatalogResults();
          bindings.setDirty(true);
          showNotice(t("models.apiKeyPasted"));
        }
      } catch (err) {
        // clipboard permission denied or unsupported
      }
    });
  }
}

function bindCatalogEvents(bindings: ProviderEditorBindings): void {
  element<HTMLButtonElement>("#back-to-config").addEventListener("click", () => {
    switchToStep("config");
  });
  element<HTMLButtonElement>("#step-node-preset").addEventListener("click", () => {
    if (!element<HTMLButtonElement>("#step-node-preset").disabled) {
      switchToStep("preset");
    }
  });
  element<HTMLButtonElement>("#step-node-config").addEventListener("click", () => {
    if (!element<HTMLButtonElement>("#step-node-config").disabled) {
      switchToStep("config");
    }
  });
  element<HTMLButtonElement>("#step-node-catalog").addEventListener("click", () => {
    if (!element<HTMLButtonElement>("#step-node-catalog").disabled) {
      switchToStep("catalog");
    }
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
  for (const selector of ["#close-provider-modal", "#provider-modal-backdrop", "#cancel-provider", "#cancel-provider-step1"]) {
    const btn = document.querySelector<HTMLElement>(selector);
    if (btn) btn.addEventListener("click", () => void bindings.closeEditor());
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
