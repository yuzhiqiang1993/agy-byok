import type { IdeStatus } from "../types/host";
import { element, setButtonUnavailable } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import {
  disableIdeIntegration,
  enableIdeIntegration,
  launchIde,
  refreshIde,
  openPath,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { t } from "../i18n";
import { setupHostIntegrationActions } from "./host/HostIntegrationActions";

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
  launchIdeButton.hidden = !status.canLaunchIde;
  const launchLabel = status.ideRunning ? "overview.restart" : "overview.launch";
  launchIdeButton.dataset.i18n = launchLabel;
  launchIdeButton.textContent = t(launchLabel);
  disableIdeIntegrationButton.hidden = !status.canDisableIntegration;
  disableIdeIntegrationButton.textContent = t("overview.disableIntegration");

  const canEnable = status.canEnableIntegration && status.proxyRunning;
  enableIdeIntegrationButton.title = !status.proxyRunning
    ? t("overview.hostProxyRequired")
    : "";
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
  setupHostIntegrationActions({
    client: "ide",
    messages: "desktop",
    getCurrentStatus: () => store.ideStatus,
    isRunning: (status) => status?.ideRunning ?? false,
    enable: enableIdeIntegration,
    disable: disableIdeIntegration,
    refresh: refreshIde,
    render: renderIde,
    launch: launchIde,
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
