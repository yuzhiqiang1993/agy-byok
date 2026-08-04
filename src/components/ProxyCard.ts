import type { ProxyStatus } from "../types/proxy";
import { element, errorMessage, setButtonUnavailable, withBusy } from "../utils/domUtils";
import { store } from "../store/appStore";
import { DEFAULT_PROXY_PORT } from "../types/config";
import { startProxy as startProxyCommand, stopProxy as stopProxyCommand } from "../controllers/proxyController";
import { refreshIde, refreshApp, refreshCli } from "../controllers/hostController";
import { switchTab } from "./TabManager";
import { t } from "../i18n";
import { showNotice } from "./NoticeBar";

export function renderProxyLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLElement>("#proxy-address");
  state.textContent = t("overview.loadFailed");
  state.className = "status-pill error";
  address.textContent = t("overview.loadFailedDetail", { message });
  const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
  stopProxyButton.hidden = true;
  setButtonUnavailable(stopProxyButton, true);
}

export function renderProxy(status: ProxyStatus): void {
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLElement>("#proxy-address");
  const running = status.state === "running";
  state.textContent = running ? t("overview.proxyRunning") : t("overview.proxyStopped");
  state.className = `status-pill ${running ? "success" : "neutral"}`;
  address.textContent = status.address ?? `127.0.0.1:${store.config?.proxy_port ?? DEFAULT_PROXY_PORT}`;

  // 1. 代理状态微光脉冲呼吸灯
  const glowDot = document.querySelector("#proxy-glow-dot");
  if (glowDot) {
    if (running) {
      glowDot.classList.add("running");
    } else {
      glowDot.classList.remove("running");
    }
  }

  const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
  stopProxyButton.textContent = running ? t("overview.stopProxy") : t("overview.startProxy");
  stopProxyButton.className = running ? "secondary compact-button" : "primary compact-button";
  stopProxyButton.hidden = false;
  setButtonUnavailable(stopProxyButton, false);
}

export function proxyPortFromAddress(address: string | null): number | null {
  if (!address) return null;
  const separator = address.lastIndexOf(":");
  const port = Number(address.slice(separator + 1));
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

export async function startProxy(): Promise<void> {
  const modelCount = store.config?.virtual_models.length ?? 0;
  if (modelCount === 0) {
    showNotice(t("overview.proxyModelsRequired", { count: 1 }), "error");
    void switchTab("tab-models");
    return;
  }
  const status = await startProxyCommand();
  renderProxy(status);
  await Promise.all([refreshIde(), refreshApp(), refreshCli()]);
  showNotice(t("overview.proxyStarted"));
}

export function setupProxyCard(): void {
  const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
  stopProxyButton.addEventListener("click", () => void withBusy(stopProxyButton, async () => {
    if (store.proxyStatus?.state === "running") {
      renderProxy(await stopProxyCommand());
      const results = await Promise.allSettled([refreshIde(), refreshApp(), refreshCli()]);
      if (results.some((result) => result.status === "rejected")) {
        showNotice(t("overview.proxyStoppedRefreshFailed"), "error");
      } else {
        showNotice(t("overview.proxyStoppedNotice"));
      }
    } else {
      await startProxy();
    }
  }));

  const copyProxyAddressBtn = document.querySelector("#copy-proxy-address");
  if (copyProxyAddressBtn) {
    copyProxyAddressBtn.addEventListener("click", () => {
      const address = element<HTMLElement>("#proxy-address").textContent?.trim() ?? "";
      if (!address) return;
      const fullUrl = address.startsWith("http") ? address : `http://${address}`;
      navigator.clipboard.writeText(fullUrl).then(() => {
        showNotice(t("overview.proxyAddressCopied", { address: fullUrl }));
      }).catch((err) => {
        showNotice(t("overview.copyFailed", { message: errorMessage(err) }), "error");
      });
    });
  }
}
