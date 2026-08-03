import type { CliStatus } from "../types/host";
import { element, setButtonUnavailable, withClientBusy, withBusy } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import {
  disableCliIntegration,
  enableCliIntegration,
  refreshCli,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { switchTab } from "./TabManager";
import { t } from "../i18n";

export function renderCli(status: CliStatus): void {
  const state = element<HTMLSpanElement>("#cli-state");
  const detail = element<HTMLParagraphElement>("#cli-detail");
  state.textContent = status.installed ? t("overview.installed") : t("overview.notInstalled");
  state.className = `status-pill ${status.installed ? "neutral" : "error"}`;
  detail.textContent = status.installed ? t("overview.cliInstalled") : t("overview.msgUnavailable");

  const integrationState = element<HTMLSpanElement>("#cli-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#cli-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
    "cli",
  );

  const enableCliBtn = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliBtn = element<HTMLButtonElement>("#disable-cli-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (status.integrationState === "managed" || status.integrationState === "external")
    && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableCliBtn.hidden = !showEnableOrUpdateButton;
  enableCliBtn.textContent = needsReconfiguration ? t("overview.mismatch") : t("overview.enableIntegration");

  disableCliBtn.hidden = !status.canDisableIntegration;
  disableCliBtn.textContent = t("overview.disableIntegration");

  const modelCount = store.config?.virtual_models.length ?? 0;
  const canEnable = status.canEnableIntegration && modelCount > 0 && status.proxyRunning;
  setButtonUnavailable(enableCliBtn, !canEnable);
  setButtonUnavailable(disableCliBtn, !status.canDisableIntegration);
}

export function renderCliLoadFailure(message: string): void {
  const state = element<HTMLSpanElement>("#cli-state");
  const detail = element<HTMLParagraphElement>("#cli-detail");
  const integrationState = element<HTMLSpanElement>("#cli-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#cli-integration-detail");

  state.textContent = t("overview.loadFailed");
  state.className = "status-pill error";
  detail.textContent = t("overview.loadFailedDetail", { message });
  integrationState.textContent = t("overview.loadFailed");
  integrationState.className = "status-pill error";
  integrationDetail.textContent = t("overview.loadFailedDetail", { message });
}

export function setupCliCard(): void {
  const enableCliButton = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliButton = element<HTMLButtonElement>("#disable-cli-integration");

  enableCliButton.addEventListener("click", () => {
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
      const current = store.cliStatus;
      const needsReconfiguration = current?.integrationState === "mismatch"
        || current?.configurationState === "needs_update";
      const alreadyEnabled = current?.integrationState === "managed" && !needsReconfiguration;
      const status = await withClientBusy(enableCliButton, "cli", async () => {
        const confirmMsg = needsReconfiguration
          ? t("overview.cliUpdateConfirm")
          : alreadyEnabled
            ? t("overview.cliAlreadyEnabledConfirm")
            : t("overview.cliEnableConfirm");
        const confirmTitle = needsReconfiguration ? t("overview.hostUpdateTitle") : t("overview.hostEnableTitle");
        const confirmOk = needsReconfiguration ? t("overview.hostUpdateOk") : t("overview.hostEnableOk");
        if (!await confirmHostAction(confirmMsg, confirmTitle, confirmOk, t("overview.hostCancel"))) return null;

        showNotice(t(needsReconfiguration ? "overview.hostUpdating" : "overview.hostEnabling", { client: t("overview.clientCli") }));
        return enableCliIntegration();
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice(alreadyEnabled && status.integrationState === "managed"
          ? t("overview.cliAlreadyEnabled")
          : needsReconfiguration
            ? t("overview.cliUpdated")
            : t("overview.cliEnabled"));
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
        const confirmMsg = t("overview.cliRestoreConfirm");
        if (!await confirmHostAction(confirmMsg, t("overview.hostRestoreTitle"), t("overview.hostRestoreOk"), t("overview.hostCancel"))) return null;

        showNotice(t("overview.hostRestoring", { client: t("overview.clientCli") }));
        return disableCliIntegration();
      });
      if (status === null) return;
      if (status) {
        renderCli(status);
        showNotice(t("overview.cliRestored"));
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
