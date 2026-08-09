import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import type { ModelConnectionTestResult } from "../types/proxy";

export const providerService = {
  fetchCatalog: (provider: Provider) =>
    invoke<ProviderCatalogModel[]>("fetch_provider_catalog", { provider }),
  fetchOfficialModels: () =>
    invoke<ProviderCatalogModel[]>("fetch_official_models"),
  testModelConnection: (virtualModelId: string) =>
    invoke<ModelConnectionTestResult>("test_model_connection", { virtualModelId }),
  testProviderModelConnection: (
    provider: Provider,
    upstreamModelId: string,
    reasoningLevel: ReasoningLevel | null,
    customReasoningValue: string | null,
    reasoningMapping: ReasoningMapping | null,
  ) =>
    invoke<ModelConnectionTestResult>("test_provider_model_connection", {
      provider,
      upstreamModelId,
      reasoningLevel,
      customReasoningValue,
      reasoningMapping,
    }),
};
