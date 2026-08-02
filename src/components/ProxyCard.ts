import { invoke } from "@tauri-apps/api/core";
import type { ProxyStatus } from "../types/proxy";
import { element, errorMessage } from "../utils/domUtils";
import { store } from "../store/appStore";
import { renderReadiness } from "./ReadinessPanel";
import { refreshIde, refreshApp, refreshCli } from "./HostRefresh";
import { showNotice } from "./NoticeBar";
import { setButtonUnavailable, withBusy } from "../utils/domUtils";

export function renderProxy(status: ProxyStatus): void {
  store.setProxyStatus(status);
  const actualPort = proxyPortFromAddress(status.address);
  if (actualPort !== null) {
    if (store.config) {
        store.config.proxy_port = actualPort;
    }
  }
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLElement>("#proxy-address");
  const running = status.state === "running";
  state.textContent = running ? "运行中" : "已停止";
  state.className = `status-pill ${running ? "success" : "neutral"}`;
  address.textContent = status.address ?? `127.0.0.1:${store.config?.proxy_port ?? 51234}`;

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
  stopProxyButton.textContent = running ? "停止代理" : "启动代理";
  stopProxyButton.className = running ? "secondary compact-button" : "primary compact-button";
  stopProxyButton.hidden = false;
  setButtonUnavailable(stopProxyButton, false);
  renderReadiness();
}

export function proxyPortFromAddress(address: string | null): number | null {
  if (!address) return null;
  const separator = address.lastIndexOf(":");
  const port = Number(address.slice(separator + 1));
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

export async function startProxy(): Promise<void> {
  const status = await invoke<ProxyStatus>("start_proxy");
  renderProxy(status);
  await Promise.all([refreshIde(), refreshApp(), refreshCli()]);
  showNotice("服务已启动");
}

export function setupProxyCard(): void {
  const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
  stopProxyButton.addEventListener("click", () => void withBusy(stopProxyButton, async () => {
    if (store.proxyStatus?.state === "running") {
      renderProxy(await invoke<ProxyStatus>("stop_proxy"));
      const results = await Promise.allSettled([refreshIde(), refreshApp(), refreshCli()]);
      if (results.some((result) => result.status === "rejected")) {
        showNotice("服务已停止，但应用状态刷新失败，请手动刷新", "error");
      } else {
        showNotice("服务已停止；已接入的模型暂时无法使用");
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
        showNotice(`已复制代理地址 ${fullUrl}`);
      }).catch((err) => {
        showNotice(`复制失败：${errorMessage(err)}`, "error");
      });
    });
  }
}
