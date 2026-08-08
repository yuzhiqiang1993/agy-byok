import type { AppConfig, Provider } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import { providerService } from "../services/providerService";
import { updateConfig } from "./configController";

export async function persistConfig(nextConfig: AppConfig): Promise<AppConfig> {
  return updateConfig((current) => ({
    ...nextConfig,
    proxy_port: current.proxy_port,
    official_model_settings: current.official_model_settings,
  }));
}

export async function removeProvider(providerId: string): Promise<AppConfig> {
  return updateConfig((current) => {
    const upstreamModels = current.upstream_models.filter(
      (model) => model.provider_id !== providerId,
    );
    const retainedUpstreamIds = new Set(upstreamModels.map((model) => model.id));
    return {
      ...current,
      providers: current.providers.filter((provider) => provider.id !== providerId),
      upstream_models: upstreamModels,
      virtual_models: current.virtual_models.filter(
        (model) => retainedUpstreamIds.has(model.upstream_model_id),
      ),
    };
  });
}

export function fetchProviderCatalog(provider: Provider): Promise<ProviderCatalogModel[]> {
  return providerService.fetchCatalog(provider);
}

export async function testVirtualModelConnection(virtualModelId: string): Promise<ModelConnectionTestResult> {
  return providerService.testModelConnection(virtualModelId);
}

export async function testProviderModelConnection(
  provider: Provider,
  upstreamModelId: string,
  reasoningLevel: ReasoningLevel | null,
  customReasoningValue: string | null,
  reasoningMapping: ReasoningMapping | null,
): Promise<ModelConnectionTestResult> {
  return providerService.testProviderModelConnection(
    provider,
    upstreamModelId,
    reasoningLevel,
    customReasoningValue,
    reasoningMapping,
  );
}
