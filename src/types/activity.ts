export interface ActivityItem {
  id: string;
  kind: string;
  operation: string;
  requestMethod: string;
  requestPath: string;
  requestBodyBytes: number | null;
  responseBodyBytes: number | null;
  responseSummary: string | null;
  timestampMs: number;
  requestedVirtualModelId: string;
  virtualModelId: string;
  upstreamModelId: string | null;
  providerId: string;
  providerProtocol: string | null;
  statusCode: number;
  durationMs: number;
  errorCategory: string | null;
  errorDetail: string | null;
  stream: boolean;
  messageCount: number;
  toolCount: number;
  usedFallback: boolean;
  fallbackAttempted: boolean;
  fallbackSucceeded: boolean;
  promptTokens: number | null;
  completionTokens: number | null;
}
