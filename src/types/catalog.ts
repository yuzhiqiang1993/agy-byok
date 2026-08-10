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
  /** 官方目录中的推荐标记。 */
  isRecommended?: boolean;
  /** 官方目录中模型是否属于 Agent 模型。 */
  isAgentModel?: boolean;
  /** 官方 Agent 模型在服务端排序中的位置。 */
  agentSortOrder?: number;
  /** 官方目录是否已将该模型标记为过时。 */
  isDeprecated?: boolean;
  /** 过时官方模型对应的新模型 ID。 */
  replacementModelId?: string;
}

export interface UpstreamCompressionPolicy {
  enabled: boolean;
  tokenThreshold: number;
  maxTokenLimit: number;
  maxOutputTokens?: number;
  checkpointModel?: string;
  useLastPlannerModel?: boolean;
}
