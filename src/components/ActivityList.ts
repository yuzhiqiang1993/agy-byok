import { invoke } from "@tauri-apps/api/core";
import type { ActivityItem } from "../types/activity";
import { store } from "../store/appStore";
import { element, errorMessage, setButtonUnavailable, withBusy } from "../utils/domUtils";
import { armDestructiveButton } from "./ProviderCard";
import { showNotice } from "./NoticeBar";
import { findVirtualModelByAcceptedId, configuredModelDisplayName } from "../utils/modelUtils";

let activityRequestVersion = 0;
let activityActionInProgress = false;
let activityRefreshInFlight: Promise<void> | null = null;
let activityItems: ActivityItem[] = [];
let activitySnapshot = "";
let activityFailedOnly = false;

function formatActivityTime(timestampMs: number): { label: string; dateTime: string | null } {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return { label: "时间未知", dateTime: null };
  return {
    label: new Intl.DateTimeFormat("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(date),
    dateTime: date.toISOString(),
  };
}

function formatDuration(durationMs: number): string {
  return durationMs >= 1000 ? `${(durationMs / 1000).toFixed(2)} s` : `${durationMs} ms`;
}

function isActivityFailure(item: ActivityItem): boolean {
  return item.statusCode < 200 || item.statusCode >= 300 || item.errorCategory !== null;
}

function providerProtocolLabel(protocol: string | null): string {
  const normalized = protocol === "openai" ? "openai_chat_completions"
    : protocol === "anthropic" ? "anthropic_messages"
      : protocol === "gemini" ? "gemini_generate_content"
        : protocol;
  if (normalized === null) return "未知";
  if (normalized === "openai_chat_completions" || normalized === "openai_responses"
    || normalized === "anthropic_messages" || normalized === "gemini_generate_content") {
    const protocols: Record<string, string> = {
      openai_chat_completions: "OpenAI · Chat Completions",
      openai_responses: "OpenAI · Responses API",
      anthropic_messages: "Anthropic · Messages API",
      gemini_generate_content: "Google · Gemini generateContent",
    };
    return protocols[normalized] ?? protocol ?? "未知";
  }
  return protocol ?? "未知";
}

function resolveActivityContext(item: ActivityItem): {
  requestedName: string;
  actualRouteName: string;
  upstreamName: string;
  providerName: string;
} {
  const resolveVirtualModelName = (virtualModelId: string): string => {
    const config = store.config;
    if (!config) return virtualModelId;
    const virtualModel = findVirtualModelByAcceptedId(config, virtualModelId);
    const upstream = virtualModel
      ? config.upstream_models.find((model) => model.id === virtualModel.upstream_model_id)
      : undefined;
    const provider = upstream
      ? config.providers.find((candidate) => candidate.id === upstream.provider_id)
      : undefined;
    return virtualModel && upstream && provider
      ? configuredModelDisplayName(
          virtualModel.display_name,
          provider.name,
          virtualModel.default_reasoning_level,
          Object.keys(upstream.capabilities.reasoning.levels).length > 0,
        )
      : virtualModelId;
  };
  const config = store.config;
  const requestedVirtualModelId = item.requestedVirtualModelId ?? item.virtualModelId;
  const actualVirtualModel = config ? findVirtualModelByAcceptedId(config, item.virtualModelId) : undefined;
  const actualUpstream = actualVirtualModel && config
    ? config.upstream_models.find((model) => model.id === actualVirtualModel.upstream_model_id)
    : undefined;
  const actualProvider = config?.providers.find(
    (candidate) => candidate.id === (actualUpstream?.provider_id ?? item.providerId),
  );
  return {
    requestedName: resolveVirtualModelName(requestedVirtualModelId),
    actualRouteName: resolveVirtualModelName(item.virtualModelId),
    upstreamName: actualUpstream?.upstream_model_id ?? item.upstreamModelId ?? "—",
    providerName: actualProvider?.name ?? item.providerId,
  };
}

function formatNumberCompact(num: number | null): string {
  if (num === null || num === undefined) return "—";
  if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`;
  if (num >= 10_000) return `${(num / 1_000).toFixed(1)}k`;
  return num.toLocaleString();
}

export function renderActivityLog(): void {
  const activityCount = element<HTMLSpanElement>("#activity-count");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const activityList = element<HTMLDivElement>("#activity-list");
  
  const failures = activityItems.filter(isActivityFailure).length;
  const visibleItems = activityFailedOnly
    ? activityItems.filter(isActivityFailure)
    : activityItems;
  activityCount.textContent = activityFailedOnly
    ? `失败 ${visibleItems.length} / 共 ${activityItems.length} 条`
    : `最近 ${activityItems.length} 条 · 失败 ${failures}`;
  activityCount.setAttribute("aria-label", activityCount.textContent);
  setButtonUnavailable(clearActivityButton, activityItems.length === 0);
  const oldScrollTop = activityList.scrollTop;
  const oldScrollHeight = activityList.scrollHeight;
  const nearTop = oldScrollTop < 24;
  activityList.replaceChildren();

  if (visibleItems.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = activityItems.length === 0
      ? "暂无调用日志。通过本地代理发起模型请求后，记录会显示在这里。"
      : "当前没有失败日志。";
    activityList.append(empty);
    return;
  }

  for (const item of visibleItems) {
    const failed = isActivityFailure(item);
    const context = resolveActivityContext(item);
    const card = document.createElement("article");
    card.className = `activity-item ${failed ? "error" : "success"}`;

    const heading = document.createElement("div");
    heading.className = "activity-item-heading";

    const mainGroup = document.createElement("div");
    mainGroup.className = "activity-item-main";

    const timestamp = document.createElement("time");
    const formattedTime = formatActivityTime(item.timestampMs);
    timestamp.className = "activity-time";
    timestamp.textContent = formattedTime.label;
    if (formattedTime.dateTime) timestamp.dateTime = formattedTime.dateTime;

    const path = document.createElement("div");
    path.className = "activity-path";

    const reqCode = document.createElement("code");
    reqCode.textContent = context.requestedName;
    reqCode.title = item.requestedVirtualModelId ?? item.virtualModelId;

    const arrow = document.createElement("span");
    arrow.className = "activity-path-arrow";
    arrow.textContent = "──➔";

    const targetCode = document.createElement("span");
    targetCode.className = "activity-path-target";
    targetCode.textContent = `${context.providerName} (${context.upstreamName})`;
    targetCode.title = `实际上游: ${context.upstreamName} / 协议: ${providerProtocolLabel(item.providerProtocol)}`;

    path.append(reqCode, arrow, targetCode);
    mainGroup.append(timestamp, path);

    const statusGroup = document.createElement("div");
    statusGroup.className = "activity-status-group";

    const latency = document.createElement("span");
    const speedClass = item.durationMs < 1000 ? "fast" : item.durationMs < 4000 ? "medium" : "slow";
    latency.className = `activity-latency ${speedClass}`;
    latency.textContent = formatDuration(item.durationMs);

    const status = document.createElement("span");
    status.className = `status-pill ${failed ? "error" : item.fallbackSucceeded ? "accent" : "success"}`;
    const httpText = item.statusCode > 0 ? String(item.statusCode) : "无响应";
    status.textContent = failed
      ? `${httpText} · 失败`
      : item.fallbackSucceeded
        ? `${httpText} · Fallback`
        : `${httpText} OK`;

    statusGroup.append(latency, status);
    heading.append(mainGroup, statusGroup);

    const pillsRow = document.createElement("div");
    pillsRow.className = "activity-pills-row";

    const providerPill = document.createElement("span");
    providerPill.className = "activity-pill";
    providerPill.textContent = `${context.providerName} / ${providerProtocolLabel(item.providerProtocol)}`;

    const typePill = document.createElement("span");
    typePill.className = "activity-pill";
    typePill.textContent = item.stream ? "流式" : "非流式";

    const countPill = document.createElement("span");
    countPill.className = "activity-pill";
    countPill.textContent = `${item.messageCount} 消息 · ${item.toolCount} 工具`;

    pillsRow.append(providerPill, typePill, countPill);

    if (item.promptTokens !== null || item.completionTokens !== null) {
      const tokenPill = document.createElement("span");
      tokenPill.className = "activity-pill accent";
      const pFormat = formatNumberCompact(item.promptTokens);
      const cFormat = formatNumberCompact(item.completionTokens);
      tokenPill.textContent = `TOKEN: ${pFormat} 输入 · ${cFormat} 输出`;
      tokenPill.title = `输入 ${item.promptTokens ?? "—"} · 输出 ${item.completionTokens ?? "—"}`;
      pillsRow.append(tokenPill);
    }

    if (item.fallbackAttempted) {
      const fbPill = document.createElement("span");
      fbPill.className = `activity-pill ${item.fallbackSucceeded ? "accent" : "warning"}`;
      fbPill.textContent = item.fallbackSucceeded ? "Fallback 降级成功" : "Fallback 降级失败";
      pillsRow.append(fbPill);
    }

    card.append(heading, pillsRow);

    if (failed) {
      const error = document.createElement("div");
      error.className = "activity-error";
      const errorHeading = document.createElement("div");
      errorHeading.className = "activity-error-heading";
      const category = document.createElement("strong");
      category.textContent = item.errorCategory ?? "未分类错误";
      const copy = document.createElement("button");
      copy.type = "button";
      copy.className = "quiet activity-copy-error";
      copy.textContent = "复制错误诊断";
      copy.addEventListener("click", () => {
        const text = [
          `时间: ${formattedTime.label}`,
          `请求模型: ${context.requestedName}`,
          `实际路由: ${context.actualRouteName}`,
          `实际上游: ${context.upstreamName}`,
          `上游服务: ${context.providerName}`,
          `HTTP: ${item.statusCode || "无响应"}`,
          `错误分类: ${item.errorCategory ?? "未分类错误"}`,
          `错误详情: ${item.errorDetail ?? "未提供错误详情"}`,
        ].join("\n");
        void navigator.clipboard.writeText(text)
          .then(() => showNotice("错误信息已复制"))
          .catch((copyError) => showNotice(`复制失败：${errorMessage(copyError)}`, "error"));
      });
      errorHeading.append(category, copy);
      const detail = document.createElement("p");
      detail.textContent = item.errorDetail ?? "未提供错误详情";
      error.append(errorHeading, detail);
      card.append(error);
    }
    activityList.append(card);
  }

  if (nearTop) {
    activityList.scrollTop = 0;
  } else {
    activityList.scrollTop = oldScrollTop + (activityList.scrollHeight - oldScrollHeight);
  }
}

export function setActivityItems(items: ActivityItem[]): void {
  activityItems = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
  activitySnapshot = JSON.stringify(activityItems);
  renderActivityLog();
}

export function setActivityLoadFailed(message: string): void {
  activityItems = [];
  activitySnapshot = "";
  const activityCount = element<HTMLSpanElement>("#activity-count");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const activityList = element<HTMLDivElement>("#activity-list");
  activityCount.textContent = "读取失败";
  activityCount.setAttribute("aria-label", "读取失败");
  setButtonUnavailable(clearActivityButton, true);
  activityList.replaceChildren();
  const error = document.createElement("p");
  error.className = "empty-state error-state";
  error.textContent = `调用日志读取失败：${message}。可点击刷新重试。`;
  activityList.append(error);
}

async function refreshActivityLog(silent = false): Promise<void> {
  if (activityRefreshInFlight) return activityRefreshInFlight;
  const requestVersion = activityRequestVersion;
  const task = (async () => {
    try {
      const items = await invoke<ActivityItem[]>("get_activity_log");
      if (requestVersion !== activityRequestVersion) return;
      const ordered = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
      const snapshot = JSON.stringify(ordered);
      if (snapshot !== activitySnapshot) setActivityItems(ordered);
    } catch (error) {
      if (!silent) throw error;
    }
  })();
  activityRefreshInFlight = task;
  try {
    await task;
  } finally {
    if (activityRefreshInFlight === task) activityRefreshInFlight = null;
  }
}

async function clearActivityLog(): Promise<void> {
  activityActionInProgress = true;
  activityRequestVersion += 1;
  try {
    await invoke<void>("clear_activity_log");
    activityRequestVersion += 1;
    setActivityItems([]);
    showNotice("内存调用日志已清空");
  } finally {
    activityActionInProgress = false;
  }
}

export function setupActivityList(): void {
  const refreshActivityButton = element<HTMLButtonElement>("#refresh-activity");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const failedActivityOnlyCheckbox = element<HTMLInputElement>("#activity-failed-only");

  refreshActivityButton.addEventListener("click", () => {
    void withBusy(refreshActivityButton, () => refreshActivityLog());
  });
  
  armDestructiveButton(
    clearActivityButton,
    "确认清空内存日志",
    () => withBusy(clearActivityButton, clearActivityLog),
  );
  
  failedActivityOnlyCheckbox.addEventListener("change", () => {
    activityFailedOnly = failedActivityOnlyCheckbox.checked;
    renderActivityLog();
  });
  
  window.setInterval(() => {
    if (document.visibilityState === "visible" && !activityActionInProgress) {
      void refreshActivityLog(true);
    }
  }, 2000);
  
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") void refreshActivityLog(true);
  });
}
