import { t } from "../i18n";
import type { ActivityItem } from "../types/activity";
import {
  activityErrorCategoryDiagnostic,
  activityErrorCategoryLabel,
  formatActivityTime,
  formatBytes,
  formatDuration,
  formatNumberCompact,
  httpOperationLabel,
  isActivityFailure,
  providerProtocolLabel,
  resolveActivityContext,
} from "../features/activity/activityPresentation";
import { errorMessage } from "../utils/errorUtils";
import { showNotice } from "./NoticeBar";

type ChatContext = ReturnType<typeof resolveActivityContext>;

function createHeading(
  item: ActivityItem,
  failed: boolean,
  formattedTime: ReturnType<typeof formatActivityTime>,
  context: ChatContext | null,
): HTMLDivElement {
  const heading = document.createElement("div");
  heading.className = "activity-item-heading";

  const mainGroup = document.createElement("div");
  mainGroup.className = "activity-item-main";

  const timestamp = document.createElement("time");
  timestamp.className = "activity-time";
  timestamp.textContent = formattedTime.label;
  if (formattedTime.dateTime) timestamp.dateTime = formattedTime.dateTime;

  const path = document.createElement("div");
  path.className = "activity-path";

  const request = document.createElement("code");
  request.textContent = item.kind === "http"
    ? `${item.requestMethod} ${item.requestPath}`
    : context?.requestedName ?? item.requestedVirtualModelId;
  request.title = item.kind === "http" ? item.requestPath : item.requestedVirtualModelId;

  const arrow = document.createElement("span");
  arrow.className = "activity-path-arrow";
  arrow.textContent = "──➔";

  const target = document.createElement("span");
  target.className = "activity-path-target";
  if (item.kind === "http") {
    target.textContent = httpOperationLabel(item.operation);
    target.title = item.operation;
  } else {
    target.textContent = `${context?.providerName ?? item.providerId} (${context?.upstreamName ?? item.upstreamModelId ?? "—"})`;
    target.title = t("activity.actualUpstream", {
      model: context?.upstreamName ?? item.upstreamModelId ?? "—",
      protocol: providerProtocolLabel(item.providerProtocol),
    });
  }

  path.append(request, arrow, target);
  mainGroup.append(timestamp, path);

  const statusGroup = document.createElement("div");
  statusGroup.className = "activity-status-group";

  const latency = document.createElement("span");
  const speedClass = item.durationMs < 1000 ? "fast" : item.durationMs < 4000 ? "medium" : "slow";
  latency.className = `activity-latency ${speedClass}`;
  latency.textContent = formatDuration(item.durationMs);

  const usedFallback = item.kind === "chat" && item.fallbackSucceeded;
  const status = document.createElement("span");
  status.className = `status-pill ${failed ? "error" : usedFallback ? "accent" : "success"}`;
  const httpText = item.statusCode > 0 ? String(item.statusCode) : t("activity.noResponse");
  status.textContent = failed
    ? t("activity.statusFailed", { code: httpText })
    : usedFallback
      ? t("activity.statusFallback", { code: httpText })
      : t("activity.statusOk", { code: httpText });

  statusGroup.append(latency, status);
  heading.append(mainGroup, statusGroup);
  return heading;
}

function createMetadata(item: ActivityItem, context: ChatContext | null): HTMLDivElement {
  const pills = document.createElement("div");
  pills.className = "activity-pills-row";

  if (item.kind === "http") {
    const requestBytes = document.createElement("span");
    requestBytes.className = "activity-pill";
    requestBytes.textContent = t("activity.requestBytes", {
      value: formatBytes(item.requestBodyBytes),
    });

    const responseBytes = document.createElement("span");
    responseBytes.className = "activity-pill";
    responseBytes.textContent = t("activity.responseBytes", {
      value: formatBytes(item.responseBodyBytes),
    });
    pills.append(requestBytes, responseBytes);

    if (item.responseSummary) {
      const summary = document.createElement("span");
      summary.className = "activity-pill accent activity-summary-pill";
      summary.textContent = item.responseSummary;
      summary.title = item.responseSummary;
      pills.append(summary);
    }
    return pills;
  }

  const provider = document.createElement("span");
  provider.className = "activity-pill";
  provider.textContent = `${context?.providerName ?? item.providerId} / ${providerProtocolLabel(item.providerProtocol)}`;

  const responseType = document.createElement("span");
  responseType.className = "activity-pill";
  responseType.textContent = item.stream ? t("activity.stream") : t("activity.nonStream");

  const counts = document.createElement("span");
  counts.className = "activity-pill";
  counts.textContent = `${t("activity.messageCount", { count: item.messageCount })} · ${t("activity.toolCount", { count: item.toolCount })}`;
  pills.append(provider, responseType, counts);

  if (item.totalTokens !== null) {
    const tokens = document.createElement("span");
    tokens.className = "activity-pill accent";
    tokens.textContent = t("activity.tokenLabel", { total: formatNumberCompact(item.totalTokens) });
    tokens.title = t("activity.tokenTitle", {
      input: item.inputTokens ?? "—",
      output: item.outputTokens ?? "—",
      cacheRead: item.cacheReadTokens ?? "—",
      cacheWrite: item.cacheWriteTokens ?? "—",
      reasoning: item.reasoningTokens ?? "—",
      total: item.totalTokens,
    });
    pills.append(tokens);
  }

  if (item.fallbackAttempted) {
    const fallback = document.createElement("span");
    fallback.className = `activity-pill ${item.fallbackSucceeded ? "accent" : "warning"}`;
    fallback.textContent = item.fallbackSucceeded
      ? t("activity.fallbackSuccess")
      : t("activity.fallbackFailure");
    pills.append(fallback);
  }
  return pills;
}

function diagnosticText(
  item: ActivityItem,
  formattedTime: ReturnType<typeof formatActivityTime>,
  context: ChatContext | null,
): string {
  const request = item.kind === "http"
    ? `${t("activity.httpRequestLabel")}: ${item.requestMethod} ${item.requestPath}`
    : `${t("activity.requestModelLabel")}: ${context?.requestedName ?? item.requestedVirtualModelId}`;
  const route = item.kind === "http"
    ? `${t("activity.httpOperationLabel")}: ${httpOperationLabel(item.operation)}`
    : `${t("activity.routeLabel")}: ${context?.actualRouteName ?? item.virtualModelId}`;
  const details = item.kind === "http"
    ? [
        `${t("activity.requestBytesLabel")}: ${formatBytes(item.requestBodyBytes)}`,
        `${t("activity.responseBytesLabel")}: ${formatBytes(item.responseBodyBytes)}`,
        ...(item.responseSummary
          ? [`${t("activity.responseSummaryLabel")}: ${item.responseSummary}`]
          : []),
      ]
    : [
        `${t("activity.upstreamModelLabel")}: ${context?.upstreamName ?? item.upstreamModelId ?? "—"}`,
        `${t("activity.providerLabel")}: ${context?.providerName ?? item.providerId}`,
      ];

  return [
    `${t("activity.timeLabel")}: ${formattedTime.label}`,
    request,
    route,
    ...details,
    t("activity.httpLabel", { code: item.statusCode || t("activity.noResponse") }),
    `${t("activity.errorCategoryLabel")}: ${activityErrorCategoryDiagnostic(item.errorCategory)}`,
    `${t("activity.errorDetailLabel")}: ${item.errorDetail ?? t("activity.missingErrorDetail")}`,
  ].join("\n");
}

function createErrorDetails(
  item: ActivityItem,
  formattedTime: ReturnType<typeof formatActivityTime>,
  context: ChatContext | null,
): HTMLDivElement {
  const error = document.createElement("div");
  error.className = "activity-error";

  const heading = document.createElement("div");
  heading.className = "activity-error-heading";
  const category = document.createElement("strong");
  category.textContent = activityErrorCategoryLabel(item.errorCategory);

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "quiet activity-copy-error";
  copy.textContent = t("activity.copyDiagnostic");
  copy.addEventListener("click", () => {
    void navigator.clipboard.writeText(diagnosticText(item, formattedTime, context))
      .then(() => showNotice(t("activity.diagnosticCopied")))
      .catch((copyError) => {
        showNotice(t("activity.copyFailed", { message: errorMessage(copyError) }), "error");
      });
  });
  heading.append(category, copy);

  const detail = document.createElement("p");
  detail.textContent = item.errorDetail ?? t("activity.missingErrorDetail");
  error.append(heading, detail);
  return error;
}

export function renderActivityItem(item: ActivityItem): HTMLElement {
  const failed = isActivityFailure(item);
  const context = item.kind === "chat" ? resolveActivityContext(item) : null;
  const formattedTime = formatActivityTime(item.timestampMs);
  const card = document.createElement("article");
  card.className = `activity-item ${failed ? "error" : "success"} ${item.kind}`;
  card.append(
    createHeading(item, failed, formattedTime, context),
    createMetadata(item, context),
  );
  if (failed) card.append(createErrorDetails(item, formattedTime, context));
  return card;
}
