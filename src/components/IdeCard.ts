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
  enableIdeIntegrationButton.textContent = "启用代理模式";
  launchIdeButton.textContent = "启动 IDE";
  disableIdeIntegrationButton.textContent = "恢复官方模式";
  setButtonUnavailable(enableIdeIntegrationButton, !status.canEnableIntegration);
  setButtonUnavailable(launchIdeButton, !status.canLaunchIde);
  setButtonUnavailable(disableIdeIntegrationButton, !status.canDisableIntegration);
  renderReadiness();
}

export function renderIdeLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");
  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");

  state.textContent = "读取失败";
  state.className = "status-pill error";
  detail.textContent = `状态读取失败：${message}`;
  integrationState.textContent = "读取失败";
  integrationState.className = "status-pill error";
  integrationDetail.textContent = `状态读取失败：${message}`;
  renderReadiness();
}

export function setupIdeCard(): void {
  const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
  const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
  const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

  enableIdeIntegrationButton.addEventListener("click", () => {
    void (async () => {
      const current = store.ideStatus;
      const isRunning = current?.ideRunning ?? false;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableIdeIntegrationButton, "ide", async () => {
        const confirmMsg = needsReconfiguration
          ? isRunning
            ? "当前 IDE 的代理配置需要更新，继续后会重新设置配置并重启 IDE。是否继续？"
            : "当前 IDE 的代理配置需要更新，继续后会重新设置配置；IDE 未运行，启动后生效。是否继续？"
          : alreadyEnabled
            ? "当前 IDE 已启用代理模式，无需重复设置。是否继续？"
            : isRunning
              ? "启用代理模式后，IDE 会自动重启使配置生效。是否继续？"
              : "启用代理模式后，IDE 即可使用本地代理。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认启用代理模式", "启用代理", "取消")) return null;
  
        showNotice("正在启用 IDE 代理模式…");
        return invoke<IdeStatus>("enable_ide_integration");
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        const stillEnabled = status.integrationState === "managed"
          && status.configurationState !== "needs_update";
        showNotice(alreadyEnabled && stillEnabled
          ? "IDE 当前已经启用代理模式，无需重复设置"
          : needsReconfiguration
            ? status.ideRunning
              ? "IDE 代理配置已更新并完成重启"
              : "IDE 代理配置已更新，启动 IDE 后生效"
            : status.ideRunning
              ? "IDE 已启用代理模式并完成重启"
              : "IDE 已启用代理模式，可以启动 IDE");
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
          ? "将移除 AGY BYOK 代理配置，恢复官方模式并重启 IDE。是否继续？"
          : "将移除 AGY BYOK 代理配置，恢复官方模式；下次启动 IDE 时生效。是否继续？";
        if (!await confirmHostAction(confirmMsg, "确认恢复官方模式", "恢复官方模式", "取消")) return null;
  
        showNotice("正在恢复 IDE 官方模式…");
        return invoke<IdeStatus>("disable_ide_integration");
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        showNotice(status.ideRunning
          ? "IDE 已恢复官方模式并完成重启"
          : "IDE 已恢复官方模式");
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
