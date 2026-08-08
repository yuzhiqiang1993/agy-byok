import type { ReasoningLevel, ReasoningMapping } from "./reasoning";

interface ProviderCatalogReasoning {
  supported?: boolean;
  levels?: ReasoningLevel[];
  mappings?: Partial<Record<ReasoningLevel, ReasoningMapping>>;
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
  thinking?: unknown;
  reasoning?: ProviderCatalogReasoning;
}
