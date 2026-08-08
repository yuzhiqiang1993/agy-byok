import type { ClientIntegrationState, ClientConfigurationState } from "../types/host";
import { t } from "../i18n";

export function integrationStateLabel(state: ClientIntegrationState): string {
  return {
    official: t("overview.disabled"),
    managed: t("overview.enabled"),
    external: t("overview.enabled"),
    mismatch: t("overview.mismatch"),
    conflict: t("overview.conflict"),
    unavailable: t("overview.notInstalled"),
  }[state];
}

export function integrationStateClass(state: ClientIntegrationState): string {
  if (state === "managed" || state === "external") return "success";
  if (state === "mismatch") return "warning";
  if (state === "conflict") return "error";
  return "neutral";
}

export function displayIntegrationState(
  integrationState: ClientIntegrationState,
  configurationState: ClientConfigurationState,
): ClientIntegrationState {
  return configurationState === "needs_update" ? "mismatch" : integrationState;
}

export function clientStatusMessage(
  integrationState: ClientIntegrationState,
  configurationState: ClientConfigurationState,
  clientType: "ide" | "app" | "cli" = "ide",
): string {
  if (configurationState === "needs_update") return t("overview.msgNeedsUpdate");
  if (configurationState === "service_stopped") return t("overview.msgServiceStopped");
  if (configurationState === "not_running") return t("overview.msgNotRunning");
  if (configurationState === "not_enabled") {
    return clientType === "cli" ? t("overview.msgCliNotEnabled") : t("overview.msgNotEnabled");
  }
  if (configurationState === "matched") {
    return clientType === "cli" ? t("overview.msgCliMatched") : t("overview.msgMatched");
  }
  if (integrationState === "conflict") return t("overview.msgConflict");
  if (integrationState === "unavailable") return t("overview.msgUnavailable");

  return t("overview.msgNotEnabled");
}

export function clientConfigurationReady(state: ClientConfigurationState, proxyRunning = false): boolean {
  if (state === "matched" || state === "not_running") {
    return true;
  }
  if (proxyRunning && state === "service_stopped") {
    return true;
  }
  return false;
}

export function clientReady(state: ClientIntegrationState): boolean {
  return state === "managed" || state === "external";
}
