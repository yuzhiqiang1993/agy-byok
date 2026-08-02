import { invoke } from "@tauri-apps/api/core";
import type { CliStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import { refreshCli } from "./HostRefresh";
import { renderReadiness } from "./ReadinessPanel";
import { store } from "../store/appStore";
import { switchTab } from "./TabManager";

export function renderCli(status: CliStatus): void {
  store.setCliStatus(status);
  const state = element<HTMLSpanElement>("#cli-state");
  const detail = element<HTMLParagraphElement>("#cli-detail");
  state.textContent = status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.installed ? "neutral" : "error"}`;
  detail.textContent = status.installed ? "Antigravity CLI 已安装" : "未找到 Antigravity CLI (agy)";

  const integrationState = element<HTMLSpanElement>("#cli-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#cli-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
  );

  const enableCliBtn = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliBtn = element<HTMLButtonElement>("#disable-cli-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (status.integrationState === "managed" || status.integrationState === "external")
    && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableCliBtn.hidden = !showEnableOrUpdateButton;
  enableCliBtn.textContent = needsReconfiguration ? "更新代理模式" : "启用代理模式";

  disableCliBtn.hidden = !status.canDisableIntegration;
  disableCliBtn.textContent = "恢复官方模式";

  const modelCount = store.config?.virtual_models.length ?? 0;
  const canEnable = status.canEnableIntegration && modelCount > 0 && status.proxyRunning;
  setButtonUnavailable(enableCliBtn, !canEnable);
  setButtonUnavailable(disableCliBtn, !status.canDisableIntegration);
  renderReadiness();
}

export function renderCliLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#cli-state");
  const detail = element<HTMLParagraphElement>("#cli-detail");
  const integrationState = element<HTMLSpanElement>("#cli-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#cli-integration-detail");

  state.textContent = "读取失败";
  state.className = "status-pill error";
  detail.textContent = `状态读取失败：${message}`;
  integrationState.textContent = "读取失败";
  integrationState.className = "status-pill error";
  integrationDetail.textContent = `状态读取失败：${message}`;
  renderReadiness();
}

export function setupCliCard(): void {
  const enableCliButton = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliButton = element<HTMLButtonElement>("#disable-cli-integration");

  enableCliButton.addEventListener("click", () => {
    void (async () => {
      const modelCount = store.config?.virtual_models.length ?? 0;
      if (modelCount === 0) {
        showNotice("请先在“模型管理”中配置至少 1 个模型，再接入应用", "error");
        void switchTab("tab-models");
        return;
      }
      if (!store.proxyStatus || store.proxyStatus.state !== "running") {
        showNotice("请先启动本地代理服务，再接入应用", "error");
        return;
      }
      const current = store.cliStatus;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableCliButton, "cli", async () => {
        const confirmMsg = needsReconfiguration
          ? "当前 CLI 的代理配置需要更新，继续后会重新设置 Shell 配置；开启新终端或重新加载 Shell 后生效。是否继续？"
          : alreadyEnabled
            ? "当前 CLI 已启用代理模式，无需重复设置。是否继续？"
            : "启用代理模式后会在 Shell 配置文件 (~/.zshrc 等) 中配置 CLOUD_CODE_URL。是否继续？";
        const confirmTitle = needsReconfiguration ? "确认更新代理模式" : "确认启用代理模式";
        const confirmOk = needsReconfiguration ? "更新代理" : "启用代理";
        if (!await confirmHostAction(confirmMsg, confirmTitle, confirmOk, "取消")) return null;

        showNotice(needsReconfiguration ? "正在更新 CLI 代理模式…" : "正在启用 CLI 代理模式…");
        return invoke<CliStatus>("enable_cli_integration");
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice(alreadyEnabled && status.integrationState === "managed"
          ? "CLI 当前已经启用代理模式，无需重复设置"
          : needsReconfiguration
            ? "CLI 代理配置已更新；请开启新终端或重新加载 Shell"
            : "CLI 已启用代理模式；请开启新终端或重新加载 Shell");
      } else if (store.cliStatus) {
        try {
          await refreshCli();
        } catch {
          // withClientBusy already reported operation error
        }
      }
    })();
  });
  
  disableCliButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(disableCliButton, "cli", async () => {
        const confirmMsg = "将恢复 CLI 的官方 Shell 配置并移除 AGY BYOK 设置。已打开的终端需要重新加载 Shell 配置。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认恢复官方模式", "恢复官方模式", "取消")) return null;
  
        showNotice("正在恢复 CLI 官方模式…");
        return invoke<CliStatus>("disable_cli_integration");
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice("CLI 已恢复官方模式；请重新加载 Shell 配置");
      } else if (store.cliStatus) {
        try {
          await refreshCli();
        } catch {
          // withClientBusy already reported operation error
        }
      }
    })();
  });
  
  element<HTMLButtonElement>("#refresh-cli").addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    void withBusy(button, refreshCli);
  });
}
