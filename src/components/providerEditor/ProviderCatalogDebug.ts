import { fetchProviderCatalogDebug } from "../../controllers/providerController";
import { t } from "../../i18n";
import type { Provider } from "../../types/config";
import type { ProviderCatalogDebugResult } from "../../types/providerDebug";
import { errorMessage } from "../../utils/errorUtils";
import { createModal } from "../common/Modal";
import { showNotice } from "../NoticeBar";

function formatJson(value: string): string {
  if (!value.trim()) return "";
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
}

function showDebugResult(result: ProviderCatalogDebugResult): void {
  const content = document.createElement("div");
  content.className = "raw-config-body";

  const pre = document.createElement("pre");
  pre.className = "raw-config-json";
  pre.tabIndex = 0;
  const output = formatJson(result.responseBody)
    || result.errorMessage
    || t("models.debugCatalogEmptyBody");
  pre.textContent = output;
  content.append(pre);

  createModal({
    title: t(result.success ? "models.debugCatalogTitle" : "models.debugCatalogErrorTitle"),
    body: content,
    dialogClassName: "raw-config-dialog",
    bodyClassName: "raw-config-modal-body",
    okLabel: t("models.debugCatalogCopy"),
    cancelLabel: t("modal.close"),
    onCancel: () => {},
    onOk: async () => {
      try {
        await navigator.clipboard.writeText(output);
        showNotice(t("models.debugCatalogCopied"), "success");
      } catch (error) {
        showNotice(t("overview.copyFailed", { message: errorMessage(error) }), "error");
      }
    },
  });
}

interface ProviderCatalogDebugContext {
  providerFromForm: () => Provider;
  withBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
}

export function setupProviderCatalogDebug(context: ProviderCatalogDebugContext): void {
  if (!import.meta.env.DEV) return;

  const footer = document.querySelector<HTMLElement>("#provider-step-config .modal-footer-bar");
  if (!footer || footer.querySelector("#debug-fetch-provider-models")) return;

  const button = document.createElement("button");
  button.id = "debug-fetch-provider-models";
  button.type = "button";
  button.className = "secondary";
  button.dataset.i18n = "models.debugCatalogButton";
  button.textContent = t("models.debugCatalogButton");
  button.addEventListener("click", () => {
    const form = document.querySelector<HTMLFormElement>("#provider-form");
    if (!form?.reportValidity()) return;
    void context.withBusy(button, async () => {
      showDebugResult(await fetchProviderCatalogDebug(context.providerFromForm()));
    }, t("models.debugCatalogFetching"));
  });

  footer.prepend(button);
}
