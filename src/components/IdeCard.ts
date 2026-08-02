import { invoke } from "@tauri-apps/api/core";
import type { IdeStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, errorMessage, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import { refreshIde } from "./HostRefresh";
import { renderReadiness } from "./ReadinessPanel";
import { store } from "../store/appStore";

export function renderIde(status: IdeStatus): void {
  store.setIdeStatus(status);
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");

  state.textContent = status.ideRunning ? "运行中" : status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.ideRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = !status.installed
    ? "未找到 Antigravity IDE"
    : !status.compatible
      ? "当前版本暂时无法使用"
      : status.ideRunning
        ? "Antigravity IDE 正在运行"
        : "Antigravity IDE 已安装，当前未运行";

  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
  );

  const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
  const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
  const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

  enableIdeIntegrationButton.hidden = !status.canEnableIntegration;
  launchIdeButton.hidden = !status.canLaunchIde || status.ideRunning;
  disableIdeIntegrationButton.hidden = !status.canDisableIntegration;
  enableIdeIntegrationButton.textContent = status.ideRunning ? "接入并重启" : "接入模型";
  launchIdeButton.textContent = "启动 IDE";
  disableIdeIntegrationButton.textContent = status.ideRunning ? "断开并重启" : "断开接入";
  setButtonUnavailable(enableIdeIntegrationButton, !status.canEnableIntegration);
  setButtonUnavailable(launchIdeButton, !status.canLaunchIde);
  setButtonUnavailable(disableIdeIntegrationButton, !status.canDisableIntegration);
  renderReadiness();
}

export function setupIdeCard(): void {
  const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
  const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
  const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

  enableIdeIntegrationButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(enableIdeIntegrationButton, "ide", async () => {
        const isRunning = store.ideStatus?.ideRunning ?? false;
        const confirmMsg = isRunning
          ? "接入模型后，IDE 会自动重启使配置生效。是否继续？"
          : "接入模型后，IDE 即可调用已配置的自定义模型。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认接入 Antigravity IDE", "确认接入", "取消")) return null;
  
        showNotice("正在配置 IDE 接入…");
        return invoke<IdeStatus>("enable_ide_integration");
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        showNotice(status.ideRunning
          ? "IDE 已启用模型并完成重启"
          : "IDE 已启用模型，可以启动 IDE");
      } else if (store.ideStatus) {
        try {
          await refreshIde();
        } catch {
          // withClientBusy already reported the operation error.
        }
      }
    })();
  });
  
  launchIdeButton.addEventListener("click", () => {
    void withClientBusy(launchIdeButton, "ide", async () => {
      await invoke<void>("launch_ide");
      showNotice("已启动 IDE");
      window.setTimeout(() => void refreshIde().catch(() => undefined), 700);
    }, "启动中…");
  });
  
  disableIdeIntegrationButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(disableIdeIntegrationButton, "ide", async () => {
        const isRunning = store.ideStatus?.ideRunning ?? false;
        const confirmMsg = isRunning
          ? "断开接入后，IDE 会自动重启并恢复连通官方 Cloud Code。是否继续？"
          : "断开接入后，IDE 下次启动时将恢复官方模型。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认断开 Antigravity IDE 接入", "确认断开", "取消")) return null;
  
        showNotice("正在断开 IDE 接入…");
        return invoke<IdeStatus>("disable_ide_integration");
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        showNotice(status.ideRunning
          ? "IDE 已停用模型并完成重启"
          : "IDE 已停用模型");
      } else if (store.ideStatus) {
        try {
          await refreshIde();
        } catch {
          // withClientBusy already reported the operation error.
        }
      }
    })();
  });

  element<HTMLButtonElement>("#refresh-ide").addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    void withBusy(button, refreshIde);
  });

  const openIdeSettingsBtn = document.querySelector("#open-ide-settings");
  if (openIdeSettingsBtn) {
    openIdeSettingsBtn.addEventListener("click", () => {
      const path = store.ideStatus?.settingsPath || document.querySelector("#ide-settings-path-display")?.textContent?.trim();
      if (!path) {
        showNotice("配置文件路径未知", "error");
        return;
      }
      invoke<void>("open_path", { path })
        .then(() => {
          showNotice("已在系统默认编辑器中打开配置文件");
        })
        .catch((err) => {
          showNotice(`打开配置文件失败：${errorMessage(err)}`, "error");
        });
    });
  }

  const copyIdeSettingsPathBtn = document.querySelector("#copy-ide-settings-path");
  if (copyIdeSettingsPathBtn) {
    copyIdeSettingsPathBtn.addEventListener("click", () => {
      const path = store.ideStatus?.settingsPath || document.querySelector("#ide-settings-path-display")?.textContent?.trim();
      if (!path) return;
      navigator.clipboard.writeText(path).then(() => {
        showNotice(`已复制配置文件路径`);
      }).catch((err) => {
        showNotice(`复制失败：${errorMessage(err)}`, "error");
      });
    });
  }
}
