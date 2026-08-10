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
  selectAndSetCustomIdePath,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { t } from "../i18n";
import { setupHostIntegrationActions } from "./host/HostIntegrationActions";

export function renderIde(status: IdeStatus): void {
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");

  if (status.ideRunning) {
    state.textContent = t("overview.running");
    state.className = "status-pill success";
    state.title = "";
    state.removeAttribute("role");
  } else if (status.installed) {
    state.textContent = t("overview.installed");
    state.className = "status-pill neutral";
    state.title = "";
    state.removeAttribute("role");
  } else {
    state.textContent = t("overview.notDetectedManualSetup");
    state.className = "status-pill warning clickable";
    state.title = t("overview.selectInstallPath");
    state.setAttribute("role", "button");
  }

  const baseDetail = !status.installed
    ? t("overview.msgUnavailable")
    : !status.compatible
      ? t("overview.versionMismatch")
      : status.ideRunning
        ? t("overview.ideRunning")
        : t("overview.ideNotRunning");
  detail.textContent = status.isCustomPath
    ? `${baseDetail} ${t("overview.customPathTag")}`
    : baseDetail;

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

  const ideStatePill = document.querySelector<HTMLSpanElement>("#ide-state");
  if (ideStatePill) {
    ideStatePill.addEventListener("click", async () => {
      if (store.ideStatus?.installed) return;
      try {
        const result = await selectAndSetCustomIdePath();
        if (result) {
          showNotice(t("overview.pathSetSuccess", { name: "Antigravity IDE" }));
        }
      } catch (err) {
        showNotice(errorMessage(err), "error");
      }
    });
  }

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
