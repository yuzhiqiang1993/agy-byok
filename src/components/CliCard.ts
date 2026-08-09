import type { CliStatus } from "../types/host";
import { element, setButtonUnavailable } from "../utils/domUtils";
import { integrationStateLabel, integrationStateClass, clientStatusMessage, displayIntegrationState } from "../utils/displayUtils";
import {
  disableCliIntegration,
  enableCliIntegration,
  refreshCli,
} from "../controllers/hostController";
import { store } from "../store/appStore";
import { t } from "../i18n";
import { setupHostIntegrationActions } from "./host/HostIntegrationActions";

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
    "cli",
  );

  const enableCliBtn = element<HTMLButtonElement>("#enable-cli-integration");
  const disableCliBtn = element<HTMLButtonElement>("#disable-cli-integration");

  const needsReconfiguration = visibleIntegrationState === "mismatch"
    || status.configurationState === "needs_update";
  const isManagedAndNormal = (
    status.integrationState === "managed"
      || (status.integrationState === "external" && !status.canEnableIntegration)
  ) && !needsReconfiguration;

  const showEnableOrUpdateButton = !isManagedAndNormal && (status.canEnableIntegration || status.installed);
  enableCliBtn.hidden = !showEnableOrUpdateButton;
  enableCliBtn.textContent = needsReconfiguration ? t("overview.mismatch") : t("overview.enableIntegration");

  disableCliBtn.hidden = !status.canDisableIntegration;
  disableCliBtn.textContent = t("overview.disableIntegration");

  const canEnable = status.canEnableIntegration && status.proxyRunning;
  enableCliBtn.title = !status.proxyRunning
    ? t("overview.hostProxyRequired")
    : "";
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
  setupHostIntegrationActions({
    client: "cli",
    messages: "cli",
    getCurrentStatus: () => store.cliStatus,
    isRunning: () => false,
    enable: enableCliIntegration,
    disable: disableCliIntegration,
    refresh: refreshCli,
    render: renderCli,
    integrationRemainsActiveAfterDisable: (status) => status.integrationState === "external",
  });
}
