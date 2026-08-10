import type { AppStatus } from "../types/host";
import { element, setButtonUnavailable } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
import {
  disableAppIntegration,
  enableAppIntegration,
  launchApp,
  refreshApp,
  selectAndSetCustomAppPath,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { t } from "../i18n";
import { setupHostIntegrationActions } from "./host/HostIntegrationActions";

export function renderApp(status: AppStatus): void {
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");

  if (status.appRunning) {
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

  const baseDetail = status.appRunning
    ? t("overview.appRunning")
    : status.installed
      ? t("overview.appNotRunning")
      : t("overview.msgUnavailable");
  detail.textContent = status.isCustomPath
    ? `${baseDetail} ${t("overview.customPathTag")}`
    : baseDetail;

  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
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
  launchAppBtn.hidden = !status.canLaunchApp;
  disableAppBtn.hidden = !status.canDisableIntegration;
  const launchLabel = status.appRunning ? "overview.restart" : "overview.launch";
  launchAppBtn.dataset.i18n = launchLabel;
  launchAppBtn.textContent = t(launchLabel);
  disableAppBtn.textContent = t("overview.disableIntegration");

  const canEnable = status.canEnableIntegration && status.proxyRunning;
  enableAppBtn.title = !status.proxyRunning
    ? t("overview.hostProxyRequired")
    : "";
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
  setupHostIntegrationActions({
    client: "app",
    messages: "desktop",
    getCurrentStatus: () => store.appStatus,
    isRunning: (status) => status?.appRunning ?? false,
    enable: enableAppIntegration,
    disable: disableAppIntegration,
    refresh: refreshApp,
    render: renderApp,
    launch: launchApp,
  });

  const appStatePill = document.querySelector<HTMLSpanElement>("#app-state");
  if (appStatePill) {
    appStatePill.addEventListener("click", async () => {
      if (store.appStatus?.installed) return;
      const confirmed = await confirmHostAction(
        t("overview.selectPathGuideAppMessage"),
        t("overview.selectPathGuideAppTitle"),
        t("overview.browsePath"),
        t("overview.hostCancel"),
      );
      if (!confirmed) return;
      try {
        const result = await selectAndSetCustomAppPath();
        if (result) {
          showNotice(t("overview.pathSetSuccess", { name: "Antigravity App" }));
        }
      } catch (err) {
        showNotice(errorMessage(err), "error");
      }
    });
  }
}
