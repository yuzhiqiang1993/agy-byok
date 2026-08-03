import type { AppStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import {
  disableAppIntegration,
  enableAppIntegration,
  launchApp,
  refreshApp,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { switchTab } from "./TabManager";
import { t } from "../i18n";

export function renderApp(status: AppStatus): void {
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");
  state.textContent = status.appRunning ? t("overview.running") : status.installed ? t("overview.installed") : t("overview.notInstalled");
  state.className = `status-pill ${status.appRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = status.appRunning
    ? t("overview.appRunning")
    : status.installed
      ? t("overview.appNotRunning")
      : t("overview.msgUnavailable");

  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
    "app",
  );

  const enableAppBtn = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppBtn = element<HTMLButtonElement>("#launch-app");
  const disableAppBtn = element<HTMLButtonElement>("#disable-app-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (status.integrationState === "managed" || status.integrationState === "external")
    && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableAppBtn.hidden = !showEnableOrUpdateButton;
  enableAppBtn.textContent = needsReconfiguration ? t("overview.mismatch") : t("overview.enableIntegration");
  launchAppBtn.hidden = !status.canLaunchApp || status.appRunning;
  disableAppBtn.hidden = !status.canDisableIntegration;
  launchAppBtn.textContent = t("overview.launch");
  disableAppBtn.textContent = t("overview.disableIntegration");

  const modelCount = store.config?.virtual_models.length ?? 0;
  const canEnable = status.canEnableIntegration && modelCount > 0 && status.proxyRunning;
  setButtonUnavailable(enableAppBtn, !canEnable);
  setButtonUnavailable(launchAppBtn, !status.canLaunchApp);
  setButtonUnavailable(disableAppBtn, !status.canDisableIntegration);
}

export function renderAppLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");
  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");

  state.textContent = t("overview.loadFailed");
  state.className = "status-pill error";
  detail.textContent = t("overview.loadFailedDetail", { message });
  integrationState.textContent = t("overview.loadFailed");
  integrationState.className = "status-pill error";
  integrationDetail.textContent = t("overview.loadFailedDetail", { message });
}

export function setupAppCard(): void {
  const enableAppButton = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppButton = element<HTMLButtonElement>("#launch-app");
  const disableAppButton = element<HTMLButtonElement>("#disable-app-integration");

  enableAppButton.addEventListener("click", () => {
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
      const current = store.appStatus;
      const isRunning = current?.appRunning ?? false;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableAppButton, "app", async () => {
        const confirmMsg = needsReconfiguration
          ? isRunning
            ? t("overview.hostUpdateConfirmRunning", { client: t("overview.clientApp") })
            : t("overview.hostUpdateConfirmStopped", { client: t("overview.clientApp") })
          : alreadyEnabled
            ? t("overview.hostAlreadyEnabledConfirm", { client: t("overview.clientApp") })
            : isRunning
              ? t("overview.hostEnableConfirmRunning", { client: t("overview.clientApp") })
              : t("overview.hostEnableConfirmStopped", { client: t("overview.clientApp") });
        const confirmTitle = needsReconfiguration ? t("overview.hostUpdateTitle") : t("overview.hostEnableTitle");
        const confirmOk = needsReconfiguration ? t("overview.hostUpdateOk") : t("overview.hostEnableOk");
        if (!await confirmHostAction(confirmMsg, confirmTitle, confirmOk, t("overview.hostCancel"))) return null;

        showNotice(t(needsReconfiguration ? "overview.hostUpdating" : "overview.hostEnabling", { client: t("overview.clientApp") }));
        return enableAppIntegration();
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        const stillEnabled = status.integrationState === "managed"
          && status.configurationState !== "needs_update";
        showNotice(alreadyEnabled && stillEnabled
          ? t("overview.hostAlreadyEnabled", { client: t("overview.clientApp") })
          : needsReconfiguration
            ? status.appRunning
              ? t("overview.hostUpdatedRunning", { client: t("overview.clientApp") })
              : t("overview.hostUpdatedStopped", { client: t("overview.clientApp") })
            : status.appRunning
              ? t("overview.hostEnabledRunning", { client: t("overview.clientApp") })
              : t("overview.hostEnabledStopped", { client: t("overview.clientApp") }));
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
      await launchApp();
      showNotice(t("overview.hostLaunched", { client: t("overview.clientApp") }));
      window.setTimeout(() => void refreshApp().catch(() => undefined), 700);
    }, t("overview.hostLaunching", { client: t("overview.clientApp") }));
  });
  
  disableAppButton.addEventListener("click", () => {
    void (async () => {
      const status = await withClientBusy(disableAppButton, "app", async () => {
        const isRunning = store.appStatus?.appRunning ?? false;
        const confirmMsg = isRunning
          ? t("overview.hostRestoreConfirmRunning", { client: t("overview.clientApp") })
          : t("overview.hostRestoreConfirmStopped", { client: t("overview.clientApp") });
        if (!await confirmHostAction(confirmMsg, t("overview.hostRestoreTitle"), t("overview.hostRestoreOk"), t("overview.hostCancel"))) return null;

        showNotice(t("overview.hostRestoring", { client: t("overview.clientApp") }));
        return disableAppIntegration();
      });
      if (status === null) return;
      if (status) {
        renderApp(status);
        showNotice(status.appRunning
          ? t("overview.hostRestoredRunning", { client: t("overview.clientApp") })
          : t("overview.hostRestoredStopped", { client: t("overview.clientApp") }));
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
