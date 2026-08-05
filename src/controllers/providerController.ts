import type { AppConfig, Provider } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import { configService } from "../services/configService";
import { providerService } from "../services/providerService";
import { store } from "../store/appStore";
import { t } from "../i18n";

export async function persistConfig(nextConfig: AppConfig): Promise<AppConfig> {
  if (!store.configLoaded) {
    throw new Error(store.configLoadError ?? t("overview.loadFailed"));
  }
  const savedConfig = await configService.saveConfig(nextConfig);
  store.setConfig(savedConfig);
  return savedConfig;
}

export async function removeProvider(providerId: string): Promise<AppConfig> {
  if (!store.configLoaded) {
    throw new Error(store.configLoadError ?? t("overview.loadFailed"));
  }
  const config = store.config;
  const nextConfig: AppConfig = {
    ...config,
    providers: config.providers.filter((provider) => provider.id !== providerId),
    upstream_models: config.upstream_models.filter((model) => model.provider_id !== providerId),
    virtual_models: [],
  };
  const retainedUpstreamIds = new Set(nextConfig.upstream_models.map((model) => model.id));
  nextConfig.virtual_models = config.virtual_models.filter((model) => retainedUpstreamIds.has(model.upstream_model_id));
  return persistConfig(nextConfig);
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
