import { fetchOfficialModelsDebug } from "../controllers/providerController";
import { t } from "../i18n";
import type { OfficialModelsDebugResult } from "../types/officialModelsDebug";
import { errorMessage } from "../utils/errorUtils";
import { withBusy } from "../utils/domUtils";
import { createModal } from "./common/Modal";
import { showNotice } from "./NoticeBar";

function formatJson(value: string): string {
  if (!value.trim()) return "";
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    // 非 JSON 错误正文仍保持原文，避免调试信息被吞掉或重组。
    return value;
  }
}

function showOfficialModelsDebug(
  result: OfficialModelsDebugResult,
  focus: "raw" | "modified",
): void {
  const source = focus === "raw"
    ? (result.rawResponse ?? "")
    : (result.modifiedResponse ?? "");
  const output = formatJson(source)
    || result.errorMessage
    || t("models.debugOfficialEmpty");
  const body = document.createElement("div");
  body.className = "raw-config-body";
  if (!result.success) {
    const metadata = document.createElement("pre");
    metadata.className = "raw-config-json official-debug-json";
    metadata.textContent = JSON.stringify({
      statusCode: result.statusCode,
      errorCategory: result.errorCategory,
      errorMessage: result.errorMessage,
    }, null, 2);
    body.append(metadata);
  }
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
        showOfficialModelsDebug(result, focus);
      } catch (error) {
        showNotice(errorMessage(error), "error");
      }
    }, showNotice, t("models.debugOfficialFetching"));
  });
  return button;
}

export function createOfficialModelsDebugButtons(): HTMLButtonElement[] {
  if (!import.meta.env.DEV) return [];
  return [
    createDebugButton("models.debugOfficialRawButton", "raw"),
    createDebugButton("models.debugOfficialModifiedButton", "modified"),
  ];
}
