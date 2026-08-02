import type { ClientIntegrationState, ClientConfigurationState } from "../types/host";
import type { ProviderProtocol } from "../types/config";
import { errorMessage } from "./domUtils";

export function integrationStateLabel(state: ClientIntegrationState): string {
  return {
    official: "官方模式",
    managed: "代理模式",
    external: "外部配置",
    mismatch: "需要更新",
    conflict: "无法修改",
    unavailable: "未找到应用",
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
  configurationMessage: string,
): string {
  if (configurationMessage) return configurationMessage;
  if (configurationState === "needs_update") return "代理配置需要更新，请重新设置";
  if (configurationState === "service_stopped") return "代理模式已配置，请先启动本地代理";
  if (configurationState === "not_running") return "代理配置正常，启动应用后生效";
  if (configurationState === "checking") return "正在检查配置…";
  if (configurationState === "not_enabled") return "当前使用官方配置，可随时启用代理模式";
  if (configurationState === "matched") return "代理配置正常";
  if (integrationState === "conflict") return "暂时无法修改，请关闭应用后刷新再试";
  return "未找到应用";
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
    return "请先启动本地代理。";
  }
  if (/App 代理|IDE settings|invalid application bundle|language_server|Wrapper|settings\.json/i.test(message)) {
    return "暂时无法修改，请关闭应用后刷新再试。";
  }
  return message;
}

export function protocolName(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: "OpenAI · Chat Completions",
    openai_responses: "OpenAI · Responses API",
    anthropic_messages: "Anthropic · Messages API",
    gemini_generate_content: "Google · Gemini generateContent",
  }[protocol];
}

export function providerProtocolLabel(protocol: string | null): string {
  const normalized = protocol === "openai" ? "openai_chat_completions"
    : protocol === "anthropic" ? "anthropic_messages"
      : protocol === "gemini" ? "gemini_generate_content"
        : protocol;
  if (normalized === null) return "未知";
  if (normalized === "openai_chat_completions" || normalized === "openai_responses"
    || normalized === "anthropic_messages" || normalized === "gemini_generate_content") {
    return protocolName(normalized);
  }
  return protocol ?? "未知";
}

export function protocolDescription(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: "适用于 /v1/chat/completions 接口，支持 CPA、Sub2API 及主流 OpenAI 兼容服务网关。",
    openai_responses: "适用于 OpenAI Responses API 兼容接口（/v1/responses），请勿误选为 Chat Completions。",
    anthropic_messages: "适用于 /v1/messages 接口，支持 Anthropic 官方 API 及兼容 Messages API 的中转服务。",
    gemini_generate_content: "适用于 Google Gemini 原生 API（:generateContent），支持 /v1beta/models 接口。",
  }[protocol];
}

export function formatActivityTime(timestampMs: number): { label: string; full: string; dateTime: string | null } {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return { label: "时间未知", full: "时间未知", dateTime: null };
  const label = new Intl.DateTimeFormat("zh-CN", {
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
