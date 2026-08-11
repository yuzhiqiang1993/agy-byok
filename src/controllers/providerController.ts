import type { AppConfig, Provider } from "../types/config";
import type { ModelConnectionTestResult } from "../types/proxy";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ProviderCatalogDebugResult } from "../types/providerDebug";
import type { OfficialModelsDebugResult } from "../types/officialModelsDebug";
import type { ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import { providerService } from "../services/providerService";
import { updateConfig } from "./configController";

export async function persistProviderConfig(
  providerId: string,
  nextConfig: AppConfig,
): Promise<AppConfig> {
  return updateConfig((current) => {
    const plannedProvider = nextConfig.providers.find((provider) => provider.id === providerId);
    if (!plannedProvider) throw new Error(`Provider ${providerId} is missing from the save plan`);

    const currentProviderUpstreamIds = new Set(
      current.upstream_models
        .filter((upstream) => upstream.provider_id === providerId)
        .map((upstream) => upstream.id),
    );
    const currentUpstreamsById = new Map(
      current.upstream_models.map((upstream) => [upstream.id, upstream]),
    );
    const plannedUpstreams = nextConfig.upstream_models
      .filter((upstream) => upstream.provider_id === providerId)
      .map((upstream) => {
        const latest = currentUpstreamsById.get(upstream.id);
        // Provider 编辑器不管理压缩策略，保存时始终保留队列中的最新值。
        return latest ? { ...upstream, compression_policy: latest.compression_policy } : upstream;
      });
    const plannedUpstreamIds = new Set(plannedUpstreams.map((upstream) => upstream.id));
    const plannedVirtuals = nextConfig.virtual_models.filter(
      (virtualModel) => plannedUpstreamIds.has(virtualModel.upstream_model_id),
    );
    const providers = current.providers.some((provider) => provider.id === providerId)
      ? current.providers.map((provider) => provider.id === providerId ? plannedProvider : provider)
      : [...current.providers, plannedProvider];

    // 只替换当前 Provider 的模型切片，保留执行时最新的其他配置。
    return {
      ...current,
      providers,
      upstream_models: [
        ...current.upstream_models.filter((upstream) => upstream.provider_id !== providerId),
        ...plannedUpstreams,
      ],
      virtual_models: [
        ...current.virtual_models.filter(
          (virtualModel) => !currentProviderUpstreamIds.has(virtualModel.upstream_model_id),
        ),
        ...plannedVirtuals,
      ],
    };
  });
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

export function fetchProviderCatalogDebug(provider: Provider): Promise<ProviderCatalogDebugResult> {
  return providerService.fetchCatalogDebug(provider);
}

export function fetchOfficialModels(): Promise<ProviderCatalogModel[]> {
  return providerService.fetchOfficialModels();
}

export function fetchOfficialModelsDebug(): Promise<OfficialModelsDebugResult> {
  return providerService.fetchOfficialModelsDebug();
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
