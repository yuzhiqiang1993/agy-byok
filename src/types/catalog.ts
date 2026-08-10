import type { ReasoningLevel, ReasoningMapping } from "./reasoning";
import type { ModelCompressionPolicy } from "./config";

interface ProviderCatalogReasoning {
  supported?: boolean;
  levels?: ReasoningLevel[];
  mappings?: Partial<Record<ReasoningLevel, ReasoningMapping>>;
  thinkingBudget?: number;
  minThinkingBudget?: number;
}

export interface ProviderCatalogModel {
  id: string;
  displayName: string;
  contextWindow?: number;
  maxContextWindow?: number;
  contextLength?: number;
  autoCompactTokenLimit?: number;
  inputTokenLimit?: number;
  outputTokenLimit?: number;
  maxTokens?: number;
  tokenBudget?: number;
  capabilities?: Record<string, unknown> | unknown[];
  supportedMimeTypes?: string[];
  supportsImages?: boolean;
  supportsVideo?: boolean;
  thinking?: unknown;
  reasoning?: ProviderCatalogReasoning;
  upstreamCompression?: UpstreamCompressionPolicy;
  defaultCompressionPolicy?: ModelCompressionPolicy;
}

export interface UpstreamCompressionPolicy {
  enabled: boolean;
  tokenThreshold: number;
  maxTokenLimit: number;
  maxOutputTokens?: number;
  checkpointModel?: string;
  useLastPlannerModel?: boolean;
}
