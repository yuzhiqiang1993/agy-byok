import type { AppConfig } from "../../types/config";
import type { ProviderChangeSummary } from "../../types/proxy";

export function summarizeProviderChanges(
  currentConfig: AppConfig,
  providerId: string,
  nextConfig: AppConfig,
  unavailableCatalogModelIds: ReadonlySet<string>,
  selectedCatalogModelIds: ReadonlySet<string>,
): ProviderChangeSummary {
  const currentUpstreams = currentConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const nextUpstreams = nextConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const currentUpstreamIds = new Set(currentUpstreams.map((item) => item.id));
  const nextUpstreamIds = new Set(nextUpstreams.map((item) => item.id));
  const currentVirtuals = currentConfig.virtual_models.filter(
    (item) => currentUpstreamIds.has(item.upstream_model_id),
  );
  const nextVirtuals = nextConfig.virtual_models.filter(
    (item) => nextUpstreamIds.has(item.upstream_model_id),
  );
  const currentVirtualIds = new Set(currentVirtuals.map((item) => item.id));
  const nextVirtualIds = new Set(nextVirtuals.map((item) => item.id));
  const currentVirtualsById = new Map(
    currentConfig.virtual_models.map((model) => [model.id, model]),
  );
  const allNextVirtualIds = new Set(nextConfig.virtual_models.map((model) => model.id));
  const fallbackBlockers = nextConfig.virtual_models.flatMap((model) => {
    const fallbackId = model.fallback_virtual_model_id;
    if (!fallbackId || allNextVirtualIds.has(fallbackId)) return [];
    return [{
      source: model.display_name,
      fallback: currentVirtualsById.get(fallbackId)?.display_name ?? fallbackId,
    }];
  });

  return {
    addedUpstreamIds: nextUpstreams
      .filter((item) => !currentUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    removedUpstreamIds: currentUpstreams
      .filter((item) => !nextUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    addedVirtualModels: nextVirtuals.filter((item) => !currentVirtualIds.has(item.id)),
    removedVirtualModels: currentVirtuals.filter((item) => !nextVirtualIds.has(item.id)),
    retainedVirtualCount: nextVirtuals.filter((item) => currentVirtualIds.has(item.id)).length,
    unavailableModelIds: [...unavailableCatalogModelIds].filter((id) => selectedCatalogModelIds.has(id)),
    fallbackBlockers,
  };
}
