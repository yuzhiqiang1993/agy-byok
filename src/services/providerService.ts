import { invoke } from "@tauri-apps/api/core";
import type { ModelCompressionPolicy, Provider } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ProviderCatalogDebugResult } from "../types/providerDebug";
import type { OfficialModelsDebugResult } from "../types/officialModelsDebug";

export const providerService = {
  fetchCatalog: (provider: Provider) =>
    invoke<ProviderCatalogModel[]>("fetch_provider_catalog", { provider }),
  fetchCatalogDebug: (provider: Provider) =>
    invoke<ProviderCatalogDebugResult>("fetch_provider_catalog_debug", { provider }),
  fetchOfficialModels: () => invoke<ProviderCatalogModel[]>("fetch_official_models"),
  fetchOfficialModelsDebug: () =>
    invoke<OfficialModelsDebugResult>("fetch_official_models_debug"),
  resolveEffectiveCompressionPolicy: (
    policy: ModelCompressionPolicy,
    capacity: number | null,
    outputTokenLimit: number | null,
  ) => invoke<ModelCompressionPolicy>("resolve_effective_compression_policy", {
    policy,
    capacity,
    outputTokenLimit,
  }),
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
