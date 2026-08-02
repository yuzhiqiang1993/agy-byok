import type { ReasoningLevel } from "./reasoning";

export interface ProviderCatalogReasoning {
  supported?: boolean;
  levels?: ReasoningLevel[];
}

export interface ProviderCatalogModel {
  id: string;
  displayName: string;
  reasoning?: ProviderCatalogReasoning;
}
