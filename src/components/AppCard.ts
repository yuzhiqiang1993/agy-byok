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

  enableAppBtn.textContent = status.appRunning ? "接入并重启" : "接入模型";
  launchAppBtn.textContent = "启动 App";
  disableAppBtn.textContent = status.appRunning ? "断开并重启" : "断开接入";

  setButtonUnavailable(enableAppBtn, !status.canEnableIntegration);
  setButtonUnavailable(launchAppBtn, !status.canLaunchApp);
  setButtonUnavailable(disableAppBtn, !status.canDisableIntegration);
  renderReadiness();
}

export function setupAppCard(): void {
  const enableAppButton = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppButton = element<HTMLButtonElement>("#launch-app");
  const disableAppButton = element<HTMLButtonElement>("#disable-app-integration");

  enableAppButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(enableAppButton, "app", async () => {
        const isRunning = store.appStatus?.appRunning ?? false;
        const confirmMsg = isRunning
          ? "接入模型后，App 会自动重启使配置生效。是否继续？"
          : "接入模型后，App 即可调用已配置的自定义模型。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认接入 Antigravity App", "确认接入", "取消")) return null;
  
        showNotice("正在配置 App 接入…");
        return invoke<AppStatus>("enable_app_integration");
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        showNotice(status.appRunning
          ? "App 已启用模型并完成重启"
          : "App 已启用模型，可以启动 App");
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
          ? "断开接入后，App 会自动重启并恢复官方模式。是否继续？"
          : "断开接入后，App 下次启动时将恢复官方模型。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认断开 Antigravity App 接入", "确认断开", "取消")) return null;
  
        showNotice("正在断开 App 接入…");
        return invoke<AppStatus>("disable_app_integration");
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        showNotice(status.appRunning
          ? "App 已停用模型并完成重启"
          : "App 已停用模型");
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
