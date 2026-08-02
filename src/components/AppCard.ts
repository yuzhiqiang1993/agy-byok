import { invoke } from "@tauri-apps/api/core";
import type { AppStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import { refreshApp } from "./HostRefresh";
import { renderReadiness } from "./ReadinessPanel";
import { store } from "../store/appStore";

export function renderApp(status: AppStatus): void {
  store.setAppStatus(status);
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");
  state.textContent = status.appRunning ? "运行中" : status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.appRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = status.appRunning
    ? "Antigravity App 正在运行"
    : status.installed
      ? "Antigravity App 已安装，当前未运行"
      : "未找到 Antigravity App";

  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
  );

  const enableAppBtn = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppBtn = element<HTMLButtonElement>("#launch-app");
  const disableAppBtn = element<HTMLButtonElement>("#disable-app-integration");

  enableAppBtn.hidden = !status.canEnableIntegration;
  launchAppBtn.hidden = !status.canLaunchApp || status.appRunning;
  disableAppBtn.hidden = !status.canDisableIntegration;

  enableAppBtn.textContent = "启用代理模式";
  launchAppBtn.textContent = "启动 App";
  disableAppBtn.textContent = "恢复官方模式";

  setButtonUnavailable(enableAppBtn, !status.canEnableIntegration);
  setButtonUnavailable(launchAppBtn, !status.canLaunchApp);
  setButtonUnavailable(disableAppBtn, !status.canDisableIntegration);
  renderReadiness();
}

export function renderAppLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");
  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");

  state.textContent = "读取失败";
  state.className = "status-pill error";
  detail.textContent = `状态读取失败：${message}`;
  integrationState.textContent = "读取失败";
  integrationState.className = "status-pill error";
  integrationDetail.textContent = `状态读取失败：${message}`;
  renderReadiness();
}

export function setupAppCard(): void {
  const enableAppButton = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppButton = element<HTMLButtonElement>("#launch-app");
  const disableAppButton = element<HTMLButtonElement>("#disable-app-integration");

  enableAppButton.addEventListener("click", () => {
    void (async () => {
      const current = store.appStatus;
      const isRunning = current?.appRunning ?? false;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableAppButton, "app", async () => {
        const confirmMsg = needsReconfiguration
          ? isRunning
            ? "当前 App 的代理配置需要更新，继续后会重新设置代理配置并重启 App。是否继续？"
            : "当前 App 的代理配置需要更新，继续后会重新设置代理配置；App 未运行，启动后生效。是否继续？"
          : alreadyEnabled
            ? "当前 App 已启用代理模式，无需重复设置。是否继续？"
            : isRunning
              ? "启用代理模式后，App 会自动重启使配置生效。是否继续？"
              : "启用代理模式后，App 即可使用本地代理。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认启用代理模式", "启用代理", "取消")) return null;

        showNotice("正在启用 App 代理模式…");
        return invoke<AppStatus>("enable_app_integration");
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        const stillEnabled = status.integrationState === "managed"
          && status.configurationState !== "needs_update";
        showNotice(alreadyEnabled && stillEnabled
          ? "App 当前已经启用代理模式，无需重复设置"
          : needsReconfiguration
            ? status.appRunning
              ? "App 代理配置已更新并完成重启"
              : "App 代理配置已更新，启动 App 后生效"
            : status.appRunning
              ? "App 已启用代理模式并完成重启"
              : "App 已启用代理模式，可以启动 App");
      } else if (store.appStatus) {
        try {
          await refreshApp();
        } catch {
          // withClientBusy already reported the operation error.
        }
      }
    })();
  });
  
  launchAppButton.addEventListener("click", () => {
    void withClientBusy(launchAppButton, "app", async () => {
      await invoke<void>("launch_app");
      showNotice("已启动 App");
      window.setTimeout(() => void refreshApp().catch(() => undefined), 700);
    }, "启动中…");
  });
  
  disableAppButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(disableAppButton, "app", async () => {
        const isRunning = store.appStatus?.appRunning ?? false;
        const confirmMsg = isRunning
          ? "将移除 AGY BYOK 代理配置，恢复官方模式并重启 App。是否继续？"
          : "将移除 AGY BYOK 代理配置，恢复官方模式；下次启动 App 时生效。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认恢复官方模式", "恢复官方模式", "取消")) return null;

        showNotice("正在恢复 App 官方模式…");
        return invoke<AppStatus>("disable_app_integration");
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        showNotice(status.appRunning
          ? "App 已恢复官方模式并完成重启"
          : "App 已恢复官方模式");
      } else if (store.appStatus) {
        try {
          await refreshApp();
        } catch {
          // withClientBusy already reported the operation error.
        }
      }
    })();
  });
  
  element<HTMLButtonElement>("#refresh-app").addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    void withBusy(button, refreshApp);
  });
}
