import { invoke } from "@tauri-apps/api/core";
import type { CliStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import { refreshCli } from "./HostRefresh";
import { renderReadiness } from "./ReadinessPanel";
import { store } from "../store/appStore";

export function renderCli(status: CliStatus): void {
  store.setCliStatus(status);
  const state = element<HTMLSpanElement>("#cli-state");
  const detail = element<HTMLParagraphElement>("#cli-detail");
  state.textContent = status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.installed ? "neutral" : "error"}`;
  detail.textContent = status.installed
    ? status.cliPath
      ? `Antigravity CLI 已安装 (${status.cliPath})`
      : "Antigravity CLI 已安装"
    : "未找到 Antigravity CLI (agy)";

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

  enableCliBtn.hidden = !status.canEnableIntegration;
  disableCliBtn.hidden = !status.canDisableIntegration;

  enableCliBtn.textContent = "接入模型";
  disableCliBtn.textContent = "断开接入";

  setButtonUnavailable(enableCliBtn, !status.canEnableIntegration);
  setButtonUnavailable(disableCliBtn, !status.canDisableIntegration);
  renderReadiness();
}

export function setupCliCard(): void {
  const enableCliButton = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliButton = element<HTMLButtonElement>("#disable-cli-integration");

  enableCliButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(enableCliButton, "cli", async () => {
        const confirmMsg = "接入模型后将在 Shell 配置文件 (~/.zshrc 等) 中自动配置 CLOUD_CODE_URL 环境变量。新终端窗口即可直接调用自定义模型。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认接入 Antigravity CLI", "确认接入", "取消")) return null;
  
        showNotice("正在配置 CLI 接入…");
        return invoke<CliStatus>("enable_cli_integration");
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice("CLI 已成功接入模型；开启新终端窗口或 source Shell 配置后生效");
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
        const confirmMsg = "断开接入后将从 Shell 配置文件中安全移除 AGY BYOK 变量注入，恢复 CLI 默认运行模式。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认断开 Antigravity CLI 接入", "确认断开", "取消")) return null;
  
        showNotice("正在断开 CLI 接入…");
        return invoke<CliStatus>("disable_cli_integration");
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice("CLI 已断开模型接入");
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
