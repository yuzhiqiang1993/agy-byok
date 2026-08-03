import type { Provider, AppConfig, VirtualModel } from "./config";

export interface ProxyStatus {
  state: "running" | "stopped";
  address: string | null;
  port: number;
}

export interface ModelConnectionTestResult {
  success: boolean;
  durationMs: number;
  message: string;
}

export type ModelConnectionTestOutcome =
  | { kind: "result"; result: ModelConnectionTestResult }
  | { kind: "error"; message: string };

export type ConnectionTestViewState =
  | { status: "testing" }
  | { status: "success"; durationMs: number }
  | { status: "error"; message: string };

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
  legacyModelIds: string[];
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
