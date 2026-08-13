import type { Provider, AppConfig, VirtualModel } from "./config";

export interface ProxyStatus {
  state: "running" | "stopped";
  address: string | null;
  port: number;
}

export interface ModelConnectionTestResult {
  success: boolean;
  durationMs: number;
  errorCategory: ModelConnectionErrorCategory | null;
  statusCode: number | null;
  requestBody: string | null;
  responseBody: string | null;
  errorMessage: string | null;
}

export type ProxyErrorCategory =
  | "authentication"
  | "invalid_request"
  | "context_length_exceeded"
  | "rate_limit"
  | "model_not_found"
  | "upstream_server_error"
  | "timeout"
  | "connection_failed"
  | "stream_interrupted"
  | "unsupported_feature"
  | "internal";

export type ModelConnectionErrorCategory = ProxyErrorCategory | "invalid_configuration";

export type ModelConnectionTestOutcome =
  | { kind: "result"; result: ModelConnectionTestResult }
  | { kind: "error"; message: string };

export type ConnectionTestViewState =
  | { status: "testing" }
  | { status: "success"; durationMs: number }
  | { status: "error"; error: ModelConnectionTestResult | string };

export interface ProviderTestSession {
  targetVirtualModelIds: string[];
  completedAt: number;
}

export interface ProviderChangeSummary {
  addedUpstreamIds: string[];
  removedUpstreamIds: string[];
  addedVirtualModels: VirtualModel[];
  removedVirtualModels: VirtualModel[];
  retainedVirtualCount: number;
  unavailableModelIds: string[];
  fallbackBlockers: Array<{
    source: string;
    fallback: string;
  }>;
}

export interface ProviderSavePlan {
  provider: Provider;
  nextConfig: AppConfig;
  summary: ProviderChangeSummary;
  wasEditing: boolean;
}
