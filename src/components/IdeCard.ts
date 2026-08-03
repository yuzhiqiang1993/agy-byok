import type { IdeStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, errorMessage, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import {
  disableIdeIntegration,
  enableIdeIntegration,
  launchIde,
  refreshIde,
  openPath,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { switchTab } from "./TabManager";
import { t } from "../i18n";

export function renderIde(status: IdeStatus): void {
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");

  state.textContent = status.ideRunning ? t("overview.running") : status.installed ? t("overview.installed") : t("overview.notInstalled");
  state.className = `status-pill ${status.ideRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = !status.installed
    ? t("overview.msgUnavailable")
    : !status.compatible
      ? t("overview.versionMismatch")
      : status.ideRunning
        ? t("overview.ideRunning")
        : t("overview.ideNotRunning");

  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
    "ide",
  );

  const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
  const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
  const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (status.integrationState === "managed" || status.integrationState === "external")
    && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableIdeIntegrationButton.hidden = !showEnableOrUpdateButton;
  enableIdeIntegrationButton.textContent = needsReconfiguration ? t("overview.mismatch") : t("overview.enableIntegration");
  launchIdeButton.hidden = !status.canLaunchIde || status.ideRunning;
  launchIdeButton.textContent = t("overview.launch");
  disableIdeIntegrationButton.hidden = !status.canDisableIntegration;
  disableIdeIntegrationButton.textContent = t("overview.disableIntegration");

  const modelCount = store.config?.virtual_models.length ?? 0;
  const canEnable = status.canEnableIntegration && modelCount > 0 && status.proxyRunning;
  setButtonUnavailable(enableIdeIntegrationButton, !canEnable);
  setButtonUnavailable(launchIdeButton, !status.canLaunchIde);
  setButtonUnavailable(disableIdeIntegrationButton, !status.canDisableIntegration);
}

export function renderIdeLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");
  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");

  state.textContent = t("overview.loadFailed");
  state.className = "status-pill error";
  detail.textContent = t("overview.loadFailedDetail", { message });
  integrationState.textContent = t("overview.loadFailed");
  integrationState.className = "status-pill error";
  integrationDetail.textContent = t("overview.loadFailedDetail", { message });
}

export function setupIdeCard(): void {
  const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
  const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
  const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

  enableIdeIntegrationButton.addEventListener("click", () => {
    void (async () => {
      const modelCount = store.config?.virtual_models.length ?? 0;
      if (modelCount === 0) {
        showNotice(t("overview.hostModelsRequired", { count: 1 }), "error");
        void switchTab("tab-models");
        return;
      }
      if (!store.proxyStatus || store.proxyStatus.state !== "running") {
        showNotice(t("overview.hostProxyRequired"), "error");
        return;
      }
      const current = store.ideStatus;
      const isRunning = current?.ideRunning ?? false;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableIdeIntegrationButton, "ide", async () => {
        const confirmMsg = needsReconfiguration
          ? isRunning
            ? t("overview.hostUpdateConfirmRunning", { client: t("overview.clientIde") })
            : t("overview.hostUpdateConfirmStopped", { client: t("overview.clientIde") })
          : alreadyEnabled
            ? t("overview.hostAlreadyEnabledConfirm", { client: t("overview.clientIde") })
            : isRunning
              ? t("overview.hostEnableConfirmRunning", { client: t("overview.clientIde") })
              : t("overview.hostEnableConfirmStopped", { client: t("overview.clientIde") });
        const confirmTitle = needsReconfiguration ? t("overview.hostUpdateTitle") : t("overview.hostEnableTitle");
        const confirmOk = needsReconfiguration ? t("overview.hostUpdateOk") : t("overview.hostEnableOk");
        if (!await confirmHostAction(confirmMsg, confirmTitle, confirmOk, t("overview.hostCancel"))) return null;

        showNotice(t(needsReconfiguration ? "overview.hostUpdating" : "overview.hostEnabling", { client: t("overview.clientIde") }));
        return enableIdeIntegration();
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        const stillEnabled = status.integrationState === "managed"
          && status.configurationState !== "needs_update";
        showNotice(alreadyEnabled && stillEnabled
          ? t("overview.hostAlreadyEnabled", { client: t("overview.clientIde") })
          : needsReconfiguration
            ? status.ideRunning
              ? t("overview.hostUpdatedRunning", { client: t("overview.clientIde") })
              : t("overview.hostUpdatedStopped", { client: t("overview.clientIde") })
            : status.ideRunning
              ? t("overview.hostEnabledRunning", { client: t("overview.clientIde") })
              : t("overview.hostEnabledStopped", { client: t("overview.clientIde") }));
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
      await launchIde();
      showNotice(t("overview.hostLaunched", { client: t("overview.clientIde") }));
      window.setTimeout(() => void refreshIde().catch(() => undefined), 700);
    }, t("overview.hostLaunching", { client: t("overview.clientIde") }));
  });
  
  disableIdeIntegrationButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(disableIdeIntegrationButton, "ide", async () => {
        const isRunning = store.ideStatus?.ideRunning ?? false;
        const confirmMsg = isRunning
          ? t("overview.hostRestoreConfirmRunning", { client: t("overview.clientIde") })
          : t("overview.hostRestoreConfirmStopped", { client: t("overview.clientIde") });
        if (!await confirmHostAction(confirmMsg, t("overview.hostRestoreTitle"), t("overview.hostRestoreOk"), t("overview.hostCancel"))) return null;
  
        showNotice(t("overview.hostRestoring", { client: t("overview.clientIde") }));
        return disableIdeIntegration();
      });
      if (status === null) return;
      if (status) {
        renderIde(status);
        showNotice(status.ideRunning
          ? t("overview.hostRestoredRunning", { client: t("overview.clientIde") })
          : t("overview.hostRestoredStopped", { client: t("overview.clientIde") }));
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
        showNotice(t("overview.hostPathUnknown"), "error");
        return;
      }
      openPath(path)
        .then(() => {
          showNotice(t("overview.hostPathOpened"));
        })
        .catch((err) => {
          showNotice(t("overview.hostPathOpenFailed", { message: errorMessage(err) }), "error");
        });
    });
  }

  const copyIdeSettingsPathBtn = document.querySelector("#copy-ide-settings-path");
  if (copyIdeSettingsPathBtn) {
    copyIdeSettingsPathBtn.addEventListener("click", () => {
      const path = store.ideStatus?.settingsPath || document.querySelector("#ide-settings-path-display")?.textContent?.trim();
      if (!path) return;
      navigator.clipboard.writeText(path).then(() => {
        showNotice(t("overview.hostPathCopied"));
      }).catch((err) => {
        showNotice(t("overview.copyFailed", { message: errorMessage(err) }), "error");
      });
    });
  }
}
