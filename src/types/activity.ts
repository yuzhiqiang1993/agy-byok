import type { ProxyErrorCategory } from "./proxy";

export type ActivityErrorCategory =
  | ProxyErrorCategory
  | "official_upstream"
  | "method_not_allowed"
  | "payload_too_large"
  | "native_forwarding_unavailable"
  | "native_forwarding_failed";

export type ActivityOperation =
  | "health_check"
  | "list_models"
  | "fetch_available_models"
  | "generate"
  | "stream_generate"
  | "passthrough"
  | "cors_preflight";

export type ActivityProviderProtocol =
  | "openai_chat_completions"
  | "anthropic_messages"
  | "gemini_generate_content"
  | "openai_responses"
  | "native";

interface ActivityCommon {
  id: string;
  timestampMs: number;
  statusCode: number;
  durationMs: number;
  errorCategory: ActivityErrorCategory | null;
  errorDetail: string | null;
}

export interface ChatActivityItem extends ActivityCommon {
  kind: "chat";
  requestedVirtualModelId: string;
  virtualModelId: string;
  upstreamModelId: string | null;
  providerId: string;
  providerProtocol: ActivityProviderProtocol | null;
  stream: boolean;
  messageCount: number;
  toolCount: number;
  fallbackAttempted: boolean;
  fallbackSucceeded: boolean;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  reasoningTokens: number | null;
  totalTokens: number | null;
}

interface HttpActivityItem extends ActivityCommon {
  kind: "http";
  operation: ActivityOperation;
  requestMethod: string;
  requestPath: string;
  requestBodyBytes: number | null;
  responseBodyBytes: number | null;
  responseSummary: string | null;
}

export type ActivityItem = ChatActivityItem | HttpActivityItem;
