import type { AppStatus } from "../types/host";
import { element, setButtonUnavailable } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import {
  disableAppIntegration,
  enableAppIntegration,
  launchApp,
  refreshApp,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { t } from "../i18n";
import { setupHostIntegrationActions } from "./host/HostIntegrationActions";

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
    "app",
  );

  const enableAppBtn = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppBtn = element<HTMLButtonElement>("#launch-app");
  const disableAppBtn = element<HTMLButtonElement>("#disable-app-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (
    status.integrationState === "managed"
      || (status.integrationState === "external" && !status.canEnableIntegration)
  ) && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableAppBtn.hidden = !showEnableOrUpdateButton;
  enableAppBtn.textContent = needsReconfiguration ? t("overview.mismatch") : t("overview.enableIntegration");
  launchAppBtn.hidden = !status.canLaunchApp || status.appRunning;
  disableAppBtn.hidden = !status.canDisableIntegration;
  launchAppBtn.textContent = t("overview.launch");
  disableAppBtn.textContent = t("overview.disableIntegration");

  const modelCount = store.config.virtual_models.length;
  const canEnable = status.canEnableIntegration && modelCount > 0 && status.proxyRunning;
  enableAppBtn.title = modelCount === 0
    ? t("overview.hostModelsRequired", { count: 1 })
    : !status.proxyRunning
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
    integrationRemainsActiveAfterDisable: (status) => status.integrationState === "external",
  });
}
