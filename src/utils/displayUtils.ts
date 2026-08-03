import type { ClientIntegrationState, ClientConfigurationState } from "../types/host";
import type { ProviderProtocol } from "../types/config";
import { errorMessage } from "./errorUtils";
import { getLanguage, t } from "../i18n";

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
  configurationMessage?: string,
  clientType: "ide" | "app" | "cli" = "ide",
): string {
  if (configurationState === "needs_update") return t("overview.msgNeedsUpdate");
  if (configurationState === "service_stopped") return t("overview.msgServiceStopped");
  if (configurationState === "not_running") return t("overview.msgNotRunning");
  if (configurationState === "checking") return t("overview.msgChecking");
  if (configurationState === "not_enabled") {
    return clientType === "cli" ? t("overview.msgCliNotEnabled") : t("overview.msgNotEnabled");
  }
  if (configurationState === "matched" || configurationState === "active") {
    return clientType === "cli" ? t("overview.msgCliMatched") : t("overview.msgMatched");
  }
  if (integrationState === "conflict") return t("overview.msgConflict");
  if (integrationState === "unavailable") return t("overview.msgUnavailable");

  return configurationMessage || t("overview.msgNotEnabled");
}

export function clientConfigurationReady(state: ClientConfigurationState, proxyRunning = false): boolean {
  if (state === "matched" || state === "not_running" || state === "active") {
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

export function clientErrorMessage(error: unknown): string {
  const message = errorMessage(error);
  if (message.includes("请先启动") || message.includes("本地代理")) {
    return t("overview.proxyStartError");
  }
  if (/App 代理|IDE settings|invalid application bundle|language_server|Wrapper|settings\.json/i.test(message)) {
    return t("overview.hostModifyError");
  }
  return message;
}

export function protocolName(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: t("models.protocolOpenAI"),
    openai_responses: t("models.protocolResponses"),
    anthropic_messages: t("models.protocolAnthropic"),
    gemini_generate_content: t("models.protocolGemini"),
  }[protocol];
}

export function providerProtocolLabel(protocol: string | null): string {
  const normalized = protocol === "openai" ? "openai_chat_completions"
    : protocol === "anthropic" ? "anthropic_messages"
      : protocol === "gemini" ? "gemini_generate_content"
        : protocol;
  if (normalized === null) return t("activity.unknown");
  if (normalized === "openai_chat_completions" || normalized === "openai_responses"
    || normalized === "anthropic_messages" || normalized === "gemini_generate_content") {
    return protocolName(normalized);
  }
  return protocol ?? t("activity.unknown");
}

export function protocolDescription(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: t("models.protocolHelpOpenAI"),
    openai_responses: t("models.protocolHelpResponses"),
    anthropic_messages: t("models.protocolHelpAnthropic"),
    gemini_generate_content: t("models.protocolHelpGemini"),
  }[protocol];
}

export function formatActivityTime(timestampMs: number): { label: string; full: string; dateTime: string | null } {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) {
    const unknown = t("activity.unknownTime");
    return { label: unknown, full: unknown, dateTime: null };
  }
  const label = new Intl.DateTimeFormat(getLanguage(), {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(date);
  return {
    label,
    full: label,
    dateTime: date.toISOString(),
  };
}
