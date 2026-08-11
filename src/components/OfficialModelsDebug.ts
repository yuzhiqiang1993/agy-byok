import { fetchOfficialModelsDebug } from "../controllers/providerController";
import { t } from "../i18n";
import type { ProviderCatalogModel } from "../types/catalog";
import type { OfficialModelsDebugResult } from "../types/officialModelsDebug";
import { errorMessage } from "../utils/errorUtils";
import { withBusy } from "../utils/domUtils";
import { createModal } from "./common/Modal";
import { showNotice } from "./NoticeBar";

function formatJson(value: string): string {
  if (!value.trim()) return "";
  try {
    const parsed: unknown = JSON.parse(value);
    // GetAvailableModels 使用 RPC 信封承载真正的模型目录；IDE 使用的是其中的 response 数据。
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const response = (parsed as Record<string, unknown>).response;
      if (response && typeof response === "object" && !Array.isArray(response)
        && "models" in response) {
        return JSON.stringify(response, null, 2);
      }
    }
    return JSON.stringify(parsed, null, 2);
  } catch {
    // 非 JSON 错误正文仍保持原文，避免调试信息被吞掉或重组。
    return value;
  }
}

function showOfficialModelsDebug(
  result: OfficialModelsDebugResult,
  displayedModels: ProviderCatalogModel[],
  focus: "raw" | "modified",
): void {
  const source = focus === "raw"
    ? result.rawResponse ?? ""
    : JSON.stringify(displayedModels.length > 0 ? displayedModels : result.normalizedModels);
  const output = formatJson(source)
    || result.errorMessage
    || t("models.debugOfficialEmpty");
  const body = document.createElement("div");
  body.className = "raw-config-body";
  const pre = document.createElement("pre");
  pre.className = "raw-config-json official-debug-json";
  pre.tabIndex = 0;
  pre.textContent = output;
  body.append(pre);

  createModal({
    title: t(focus === "raw" ? "models.debugOfficialRawTitle" : "models.debugOfficialModifiedTitle"),
    body,
    dialogClassName: "raw-config-dialog",
    bodyClassName: "raw-config-modal-body",
    okLabel: t("models.debugOfficialCopy"),
    cancelLabel: t("modal.close"),
    onCancel: () => {},
    onOk: async () => {
      try {
        await navigator.clipboard.writeText(output);
        showNotice(t("models.debugOfficialCopied"), "success");
      } catch (error) {
        showNotice(t("overview.copyFailed", { message: errorMessage(error) }), "error");
      }
    },
  });
}

function createDebugButton(
  labelKey: "models.debugOfficialRawButton" | "models.debugOfficialModifiedButton",
  focus: "raw" | "modified",
  getDisplayedModels: () => ProviderCatalogModel[],
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary compact-button";
  button.dataset.i18n = labelKey;
  button.textContent = t(labelKey);
  button.addEventListener("click", () => {
    void withBusy(button, async () => {
      try {
        const result = await fetchOfficialModelsDebug();
        showOfficialModelsDebug(result, getDisplayedModels(), focus);
      } catch (error) {
        showNotice(errorMessage(error), "error");
      }
    }, showNotice, t("models.debugOfficialFetching"));
  });
  return button;
}

export function createOfficialModelsDebugButtons(
  getDisplayedModels: () => ProviderCatalogModel[],
): HTMLButtonElement[] {
  if (!import.meta.env.DEV) return [];
  return [
    createDebugButton("models.debugOfficialRawButton", "raw", getDisplayedModels),
    createDebugButton("models.debugOfficialModifiedButton", "modified", getDisplayedModels),
  ];
}
