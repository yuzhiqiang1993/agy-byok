import { getLanguage, t, type TranslationKey } from "../../i18n";
import { store } from "../../store/appStore";
import type {
  ActivityErrorCategory,
  ActivityItem,
  ActivityOperation,
  ActivityProviderProtocol,
  ChatActivityItem,
} from "../../types/activity";
import {
  configuredModelDisplayName,
  findVirtualModelByAcceptedId,
} from "../../utils/modelUtils";

const ERROR_CATEGORY_TRANSLATIONS: Record<ActivityErrorCategory, TranslationKey> = {
  authentication: "activity.errorAuthentication",
  invalid_request: "activity.errorInvalidRequest",
  context_length_exceeded: "activity.errorContextLengthExceeded",
  rate_limit: "activity.errorRateLimit",
  model_not_found: "activity.errorModelNotFound",
  upstream_server_error: "activity.errorUpstreamServer",
  timeout: "activity.errorTimeout",
  connection_failed: "activity.errorConnectionFailed",
  stream_interrupted: "activity.errorStreamInterrupted",
  unsupported_feature: "activity.errorUnsupportedFeature",
  internal: "activity.errorInternal",
  official_upstream: "activity.errorOfficialUpstream",
  method_not_allowed: "activity.errorMethodNotAllowed",
  payload_too_large: "activity.errorPayloadTooLarge",
  native_forwarding_unavailable: "activity.errorNativeForwardingUnavailable",
  native_forwarding_failed: "activity.errorNativeForwardingFailed",
};

export function activityErrorCategoryLabel(category: ActivityErrorCategory | null): string {
  return category ? t(ERROR_CATEGORY_TRANSLATIONS[category]) : t("activity.unclassifiedError");
}

export function activityErrorCategoryDiagnostic(category: ActivityErrorCategory | null): string {
  const label = activityErrorCategoryLabel(category);
  return category ? `${label} (${category})` : label;
}

export function formatActivityTime(timestampMs: number): {
  label: string;
  dateTime: string | null;
} {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) {
    return { label: t("activity.unknownTime"), dateTime: null };
  }
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

export function formatDuration(durationMs: number): string {
  return durationMs >= 1000
    ? t("activity.durationSeconds", { value: (durationMs / 1000).toFixed(2) })
    : t("activity.durationMilliseconds", { value: durationMs });
}

export function isActivityFailure(item: ActivityItem): boolean {
  return item.statusCode < 200 || item.statusCode >= 300 || item.errorCategory !== null;
}

const PROTOCOL_TRANSLATIONS: Record<ActivityProviderProtocol, TranslationKey> = {
  openai_chat_completions: "models.protocolOpenAI",
  openai_responses: "models.protocolResponses",
  anthropic_messages: "models.protocolAnthropic",
  gemini_generate_content: "models.protocolGemini",
  native: "activity.protocolNative",
};

export function providerProtocolLabel(protocol: ActivityProviderProtocol | null): string {
  if (protocol === null) return t("activity.unknown");
  return t(PROTOCOL_TRANSLATIONS[protocol]);
}

const HTTP_OPERATION_TRANSLATIONS: Record<ActivityOperation, TranslationKey> = {
  health_check: "activity.httpOperationHealth",
  list_models: "activity.httpOperationModels",
  fetch_available_models: "activity.httpOperationCatalog",
  cors_preflight: "activity.httpOperationCors",
  generate: "activity.httpOperationGenerate",
  stream_generate: "activity.httpOperationStreamGenerate",
  passthrough: "activity.httpOperationPassthrough",
};

export function httpOperationLabel(operation: ActivityOperation): string {
  return t(HTTP_OPERATION_TRANSLATIONS[operation]);
}

export function formatBytes(value: number | null): string {
  if (value === null) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(2)} MB`;
}

export function resolveActivityContext(item: ChatActivityItem): {
  requestedName: string;
  actualRouteName: string;
  upstreamName: string;
  providerName: string;
} {
  const resolveVirtualModelName = (virtualModelId: string): string => {
    const config = store.config;
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
  const actualVirtualModel = findVirtualModelByAcceptedId(config, item.virtualModelId);
  const actualUpstream = actualVirtualModel
    ? config.upstream_models.find((model) => model.id === actualVirtualModel.upstream_model_id)
    : undefined;
  const actualProvider = config.providers.find(
    (candidate) => candidate.id === (actualUpstream?.provider_id ?? item.providerId),
  );
  return {
    requestedName: resolveVirtualModelName(item.requestedVirtualModelId),
    actualRouteName: resolveVirtualModelName(item.virtualModelId),
    upstreamName: actualUpstream?.upstream_model_id ?? item.upstreamModelId ?? "—",
    providerName: actualProvider?.name ?? item.providerId,
  };
}

export function formatNumberCompact(value: number | null): string {
  if (value === null) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 10_000) return `${(value / 1_000).toFixed(1)}k`;
  return value.toLocaleString(getLanguage());
}
