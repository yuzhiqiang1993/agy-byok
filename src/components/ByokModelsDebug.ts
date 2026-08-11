import { fetchOfficialModelsDebug } from "../controllers/providerController";
import { t } from "../i18n";
import { store } from "../store/appStore";
import { withBusy } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import { createModal } from "./common/Modal";
import { showNotice } from "./NoticeBar";

interface ByokDebugResult {
  ok: boolean;
  body: string;
}

function ensureProxyRunning(): void {
  const status = store.proxyStatus;
  if (status?.state !== "running") throw new Error(t("models.debugByokProxyStopped"));
}

function formatJson(value: string): string {
  if (!value.trim()) return "";
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    // 非 JSON 错误正文保持原文，避免调试信息被吞掉或重组。
    return value;
  }
}

async function getByokModelsDebugData(): Promise<ByokDebugResult> {
  ensureProxyRunning();
  const result = await fetchOfficialModelsDebug();
  if (result.success && result.modifiedResponse) {
    return {
      ok: true,
      body: result.modifiedResponse,
    };
  }
  return {
    ok: false,
    body: result.errorMessage || t("models.debugOfficialEmpty"),
  };
}

function showModelsJson(result: ByokDebugResult): void {
  const content = document.createElement("div");
  content.className = "raw-config-body";

  const pre = document.createElement("pre");
  pre.className = "raw-config-json byok-debug-json";
  pre.tabIndex = 0;
  const output = formatJson(result.body) || result.body;
  pre.textContent = output;
  content.append(pre);

  createModal({
    title: t(result.ok ? "models.debugByokTitle" : "models.debugByokErrorTitle"),
    body: content,
    dialogClassName: "raw-config-dialog",
    bodyClassName: "raw-config-modal-body",
    okLabel: t("models.debugByokCopy"),
    cancelLabel: t("modal.close"),
    onCancel: () => {},
    onOk: async () => {
      try {
        await navigator.clipboard.writeText(output);
        showNotice(t("models.debugByokCopied"), "success");
      } catch (error) {
        showNotice(t("overview.copyFailed", { message: errorMessage(error) }), "error");
      }
    },
  });
}

export function createByokModelsDebugButton(): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary byok-models-debug-button";
  button.dataset.i18n = "models.debugByokButton";
  button.textContent = t("models.debugByokButton");
  button.addEventListener("click", () => {
    void withBusy(button, async () => {
      showModelsJson(await getByokModelsDebugData());
    }, showNotice, t("models.debugByokFetching"));
  });
  return button;
}
