import type { ActivityItem } from "../types/activity";
import { store } from "../store/appStore";
import {
  activityState,
  nextActivityRequestVersion,
  setActivityItems as setActivityItemsState,
  setActivityLoadFailed as setActivityLoadFailedState,
} from "../features/activity/activityState";
import { getActivityLog, clearActivityLog as clearActivityLogCommand, subscribeActivityCleared } from "../controllers/activityController";
import { element, armDestructiveButton, errorMessage, setButtonUnavailable, withBusy } from "../utils/domUtils";
import { showNotice } from "./NoticeBar";
import { findVirtualModelByAcceptedId, configuredModelDisplayName } from "../utils/modelUtils";
import { getLanguage, t, subscribeLanguage } from "../i18n";



subscribeLanguage(() => {
  renderActivityLog();
});

function formatActivityTime(timestampMs: number): { label: string; dateTime: string | null } {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return { label: t("activity.unknownTime"), dateTime: null };
  return {
    label: new Intl.DateTimeFormat(getLanguage(), {
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
  return durationMs >= 1000
    ? t("activity.durationSeconds", { value: (durationMs / 1000).toFixed(2) })
    : t("activity.durationMilliseconds", { value: durationMs });
}

function isActivityFailure(item: ActivityItem): boolean {
  return item.statusCode < 200 || item.statusCode >= 300 || item.errorCategory !== null;
}

function providerProtocolLabel(protocol: string | null): string {
  const normalized = protocol === "openai" ? "openai_chat_completions"
    : protocol === "anthropic" ? "anthropic_messages"
      : protocol === "gemini" ? "gemini_generate_content"
        : protocol;
  if (normalized === null) return t("activity.unknown");
  if (normalized === "openai_chat_completions") return t("models.protocolOpenAI");
  if (normalized === "openai_responses") return t("models.protocolResponses");
  if (normalized === "anthropic_messages") return t("models.protocolAnthropic");
  if (normalized === "gemini_generate_content") return t("models.protocolGemini");
  return protocol ?? t("activity.unknown");
}

function httpOperationLabel(operation: string): string {
  const labels: Record<string, string> = {
    health_check: t("activity.httpOperationHealth"),
    list_models: t("activity.httpOperationModels"),
    fetch_available_models: t("activity.httpOperationCatalog"),
    cors_preflight: t("activity.httpOperationCors"),
    generate: t("activity.httpOperationGenerate"),
    stream_generate: t("activity.httpOperationStreamGenerate"),
    passthrough: t("activity.httpOperationPassthrough"),
  };
  return labels[operation] ?? operation;
}

function formatBytes(value: number | null): string {
  if (value === null || value === undefined) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(2)} MB`;
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
  return num.toLocaleString(getLanguage());
}

export function renderActivityLog(): void {
  const activityCount = element<HTMLSpanElement>("#activity-count");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const activityList = element<HTMLDivElement>("#activity-list");
  
  if (activityState.loadError) {
    activityCount.textContent = t("overview.loadFailed");
    activityCount.setAttribute("aria-label", activityCount.textContent);
    setButtonUnavailable(clearActivityButton, true);
    activityList.replaceChildren();
    const error = document.createElement("p");
    error.className = "empty-state error-state";
    error.textContent = t("activity.logLoadFailed", { message: activityState.loadError });
    activityList.append(error);
    return;
  }

  const failures = activityState.items.filter(isActivityFailure).length;
  const visibleItems = activityState.failedOnly
    ? activityState.items.filter(isActivityFailure)
    : activityState.items;
  activityCount.textContent = activityState.failedOnly
    ? t("activity.countBadgeFiltered", { failed: visibleItems.length, total: activityState.items.length })
    : t("activity.countBadge", { total: activityState.items.length, failed: failures });
  activityCount.setAttribute("aria-label", activityCount.textContent);
  setButtonUnavailable(clearActivityButton, activityState.items.length === 0);
  const oldScrollTop = activityList.scrollTop;
  const oldScrollHeight = activityList.scrollHeight;
  const nearTop = oldScrollTop < 24;
  activityList.replaceChildren();

  if (visibleItems.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = activityState.items.length === 0
      ? t("activity.emptyDesc")
      : t("activity.emptyDescFiltered");
    activityList.append(empty);
    return;
  }

  for (const item of visibleItems) {
    const failed = isActivityFailure(item);
    const isHttp = item.kind === "http";
    const context = isHttp ? null : resolveActivityContext(item);
    const card = document.createElement("article");
    card.className = `activity-item ${failed ? "error" : "success"} ${isHttp ? "http" : "chat"}`;

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
    reqCode.textContent = isHttp
      ? `${item.requestMethod} ${item.requestPath}`
      : context?.requestedName ?? item.requestedVirtualModelId;
    reqCode.title = isHttp
      ? item.requestPath
      : item.requestedVirtualModelId ?? item.virtualModelId;

    const arrow = document.createElement("span");
    arrow.className = "activity-path-arrow";
    arrow.textContent = "──➔";

    const targetCode = document.createElement("span");
    targetCode.className = "activity-path-target";
    if (isHttp) {
      targetCode.textContent = httpOperationLabel(item.operation);
      targetCode.title = item.operation;
    } else {
      targetCode.textContent = `${context?.providerName ?? item.providerId} (${context?.upstreamName ?? item.upstreamModelId ?? "—"})`;
      targetCode.title = t("activity.actualUpstream", {
        model: context?.upstreamName ?? item.upstreamModelId ?? "—",
        protocol: providerProtocolLabel(item.providerProtocol),
      });
    }

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
    const httpText = item.statusCode > 0 ? String(item.statusCode) : t("activity.noResponse");
    status.textContent = failed
      ? t("activity.statusFailed", { code: httpText })
      : item.fallbackSucceeded
        ? t("activity.statusFallback", { code: httpText })
        : t("activity.statusOk", { code: httpText });

    statusGroup.append(latency, status);
    heading.append(mainGroup, statusGroup);

    const pillsRow = document.createElement("div");
    pillsRow.className = "activity-pills-row";

    if (isHttp) {
      const requestBytesPill = document.createElement("span");
      requestBytesPill.className = "activity-pill";
      requestBytesPill.textContent = t("activity.requestBytes", {
        value: formatBytes(item.requestBodyBytes),
      });

      const responseBytesPill = document.createElement("span");
      responseBytesPill.className = "activity-pill";
      responseBytesPill.textContent = t("activity.responseBytes", {
        value: formatBytes(item.responseBodyBytes),
      });

      pillsRow.append(requestBytesPill, responseBytesPill);
      if (item.responseSummary) {
        const summaryPill = document.createElement("span");
        summaryPill.className = "activity-pill accent activity-summary-pill";
        summaryPill.textContent = item.responseSummary;
        summaryPill.title = item.responseSummary;
        pillsRow.append(summaryPill);
      }
    } else {
      const providerPill = document.createElement("span");
      providerPill.className = "activity-pill";
      providerPill.textContent = `${context?.providerName ?? item.providerId} / ${providerProtocolLabel(item.providerProtocol)}`;

      const typePill = document.createElement("span");
      typePill.className = "activity-pill";
      typePill.textContent = item.stream ? t("activity.stream") : t("activity.nonStream");

      const countPill = document.createElement("span");
      countPill.className = "activity-pill";
      countPill.textContent = `${t("activity.messageCount", { count: item.messageCount })} · ${t("activity.toolCount", { count: item.toolCount })}`;

      pillsRow.append(providerPill, typePill, countPill);

      if (item.promptTokens !== null || item.completionTokens !== null) {
        const tokenPill = document.createElement("span");
        tokenPill.className = "activity-pill accent";
        const pFormat = formatNumberCompact(item.promptTokens);
        const cFormat = formatNumberCompact(item.completionTokens);
        tokenPill.textContent = t("activity.tokenLabel", { input: pFormat, output: cFormat });
        tokenPill.title = t("activity.tokenTitle", {
          input: item.promptTokens ?? "—",
          output: item.completionTokens ?? "—",
        });
        pillsRow.append(tokenPill);
      }

      if (item.fallbackAttempted) {
        const fbPill = document.createElement("span");
        fbPill.className = `activity-pill ${item.fallbackSucceeded ? "accent" : "warning"}`;
        fbPill.textContent = item.fallbackSucceeded
          ? t("activity.fallbackSuccess")
          : t("activity.fallbackFailure");
        pillsRow.append(fbPill);
      }
    }

    card.append(heading, pillsRow);

    if (failed) {
      const error = document.createElement("div");
      error.className = "activity-error";
      const errorHeading = document.createElement("div");
      errorHeading.className = "activity-error-heading";
      const category = document.createElement("strong");
      category.textContent = item.errorCategory ?? t("activity.unclassifiedError");
      const copy = document.createElement("button");
      copy.type = "button";
      copy.className = "quiet activity-copy-error";
      copy.textContent = t("activity.copyDiagnostic");
      copy.addEventListener("click", () => {
        const text = [
          `${t("activity.timeLabel")}: ${formattedTime.label}`,
          isHttp
            ? `${t("activity.httpRequestLabel")}: ${item.requestMethod} ${item.requestPath}`
            : `${t("activity.requestModelLabel")}: ${context?.requestedName ?? item.requestedVirtualModelId}`,
          isHttp
            ? `${t("activity.httpOperationLabel")}: ${httpOperationLabel(item.operation)}`
            : `${t("activity.routeLabel")}: ${context?.actualRouteName ?? item.virtualModelId}`,
          ...(isHttp ? [] : [
            `${t("activity.upstreamModelLabel")}: ${context?.upstreamName ?? item.upstreamModelId ?? "—"}`,
            `${t("activity.providerLabel")}: ${context?.providerName ?? item.providerId}`,
          ]),
          t("activity.httpLabel", { code: item.statusCode || t("activity.noResponse") }),
          ...(isHttp ? [
            `${t("activity.requestBytesLabel")}: ${formatBytes(item.requestBodyBytes)}`,
            `${t("activity.responseBytesLabel")}: ${formatBytes(item.responseBodyBytes)}`,
            ...(item.responseSummary ? [`${t("activity.responseSummaryLabel")}: ${item.responseSummary}`] : []),
          ] : []),
          `${t("activity.errorCategoryLabel")}: ${item.errorCategory ?? t("activity.unclassifiedError")}`,
          `${t("activity.errorDetailLabel")}: ${item.errorDetail ?? t("activity.missingErrorDetail")}`,
        ].join("\n");
        void navigator.clipboard.writeText(text)
          .then(() => showNotice(t("activity.diagnosticCopied")))
          .catch((copyError) => showNotice(t("activity.copyFailed", { message: errorMessage(copyError) }), "error"));
      });
      errorHeading.append(category, copy);
      const detail = document.createElement("p");
      detail.textContent = item.errorDetail ?? t("activity.missingErrorDetail");
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
  setActivityItemsState(items);
  renderActivityLog();
}

export function setActivityLoadFailed(message: string): void {
  setActivityLoadFailedState(message);
  renderActivityLog();
}

async function refreshActivityLog(silent = false): Promise<void> {
  if (activityState.refreshInFlight) return activityState.refreshInFlight;
  const requestVersion = activityState.requestVersion;
  const task = (async () => {
    try {
      const items = await getActivityLog();
      if (requestVersion !== activityState.requestVersion) return;
      const ordered = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
      const snapshot = JSON.stringify(ordered);
      if (snapshot !== activityState.snapshot) setActivityItems(ordered);
    } catch (error) {
      if (!silent) {
        setActivityLoadFailed(errorMessage(error));
        throw error;
      }
    }
  })();
  activityState.refreshInFlight = task;
  try {
    await task;
  } finally {
    if (activityState.refreshInFlight === task) activityState.refreshInFlight = null;
  }
}

async function clearActivityLog(): Promise<void> {
  activityState.actionInProgress = true;
  nextActivityRequestVersion();
  try {
    await clearActivityLogCommand();
    nextActivityRequestVersion();
    showNotice(t("activity.clearSuccess"));
  } finally {
    activityState.actionInProgress = false;
  }
}

export function setupActivityList(): void {
  subscribeActivityCleared(() => {
    nextActivityRequestVersion();
    renderActivityLog();
  });

  const refreshActivityButton = element<HTMLButtonElement>("#refresh-activity");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const failedActivityOnlyCheckbox = element<HTMLInputElement>("#activity-failed-only");

  refreshActivityButton.addEventListener("click", () => {
    void withBusy(refreshActivityButton, () => refreshActivityLog(), t("activity.refreshLog"));
  });
  
  armDestructiveButton(
    clearActivityButton,
    t("activity.clearConfirm"),
    () => withBusy(clearActivityButton, clearActivityLog),
  );
  
  failedActivityOnlyCheckbox.addEventListener("change", () => {
    activityState.failedOnly = failedActivityOnlyCheckbox.checked;
    renderActivityLog();
  });
  
  window.setInterval(() => {
    if (document.visibilityState === "visible" && !activityState.actionInProgress) {
      void refreshActivityLog(true);
    }
  }, 2000);
  
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") void refreshActivityLog(true);
  });
}
