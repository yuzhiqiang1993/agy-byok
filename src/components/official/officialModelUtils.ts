import { updateConfig } from "../../controllers/configController";
import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelCompressionPolicy } from "../../types/config";

export function isOfficialSourceUnavailable(error: unknown): boolean {
  const code = error instanceof Error ? error.message : String(error);
  return code === "official_models_host_not_installed"
    || code === "official_models_host_not_running";
}

export function filterMainAgentModels(models: ProviderCatalogModel[]): ProviderCatalogModel[] {
  const hasAgentMetadata = models.some((model) => model.roles !== undefined);
  const hasRecommendationMetadata = models.some((model) => model.isRecommended !== undefined);
  const filtered = hasAgentMetadata
    ? models.filter(
      (model) =>
        model.roles?.includes("agent") === true
        && model.isDeprecated !== true
        && (!hasRecommendationMetadata || model.isRecommended === true),
    )
    : models.filter((model) => model.isDeprecated !== true);

  if (!filtered.some((model) => model.agentSortOrder !== undefined)) {
    return filtered;
  }

  return filtered.sort((left, right) => {
    const leftOrder = left.agentSortOrder ?? Number.MAX_SAFE_INTEGER;
    const rightOrder = right.agentSortOrder ?? Number.MAX_SAFE_INTEGER;
    return leftOrder - rightOrder || left.id.localeCompare(right.id);
  });
}

export function officialModelAliases(models: ProviderCatalogModel[]): Map<string, string> {
  const aliases = new Map<string, string>();
  for (const model of models) {
    if (model.isDeprecated && model.replacementModelId) {
      aliases.set(model.id, model.replacementModelId);
    }
  }
  return aliases;
}

export function canonicalOfficialModelId(modelId: string, aliases: ReadonlyMap<string, string>): string {
  let canonicalId = modelId;
  const visited = new Set<string>();
  while (aliases.has(canonicalId) && !visited.has(canonicalId)) {
    visited.add(canonicalId);
    canonicalId = aliases.get(canonicalId) ?? canonicalId;
  }
  return canonicalId;
}

export function officialRelatedModelIds(
  modelId: string,
  aliases: ReadonlyMap<string, string>,
): Set<string> {
  const canonicalId = canonicalOfficialModelId(modelId, aliases);
  const relatedModelIds = new Set<string>([canonicalId]);
  for (const deprecatedId of aliases.keys()) {
    if (canonicalOfficialModelId(deprecatedId, aliases) === canonicalId) {
      relatedModelIds.add(deprecatedId);
    }
  }
  return relatedModelIds;
}

export function synchronizedOfficialPolicies(
  policies: Record<string, ModelCompressionPolicy>,
  aliases: ReadonlyMap<string, string>,
): Record<string, ModelCompressionPolicy> | null {
  const mappedModelIds = new Set([...aliases.keys(), ...aliases.values()]);
  if (![...mappedModelIds].some((modelId) => policies[modelId] !== undefined)) {
    return null;
  }

  const nextPolicies = { ...policies };
  let changed = false;
  const canonicalIds = new Set(
    [...aliases.values()].map((modelId) => canonicalOfficialModelId(modelId, aliases)),
  );
  for (const canonicalId of canonicalIds) {
    const relatedModelIds = officialRelatedModelIds(canonicalId, aliases);
    const policy = nextPolicies[canonicalId]
      ?? [...relatedModelIds]
        .map((modelId) => nextPolicies[modelId])
        .find((candidate) => candidate !== undefined);
    if (!policy) continue;
    const serializedPolicy = JSON.stringify(policy);
    for (const relatedModelId of relatedModelIds) {
      if (JSON.stringify(nextPolicies[relatedModelId]) !== serializedPolicy) {
        nextPolicies[relatedModelId] = policy;
        changed = true;
      }
    }
  }
  return changed ? nextPolicies : null;
}

export async function synchronizeOfficialModelPolicies(
  aliases: ReadonlyMap<string, string>,
): Promise<void> {
  // 接口返回的过时映射代表同一逻辑模型；已有任一侧策略时，两侧保持一致。
  if (!store.configLoaded) return;

  const nextPolicies = synchronizedOfficialPolicies(
    store.config.model_compression_policies,
    aliases,
  );
  if (!nextPolicies) return;

  await updateConfig((current) => {
    const currentPolicies = synchronizedOfficialPolicies(
      current.model_compression_policies,
      aliases,
    );
    return currentPolicies
      ? { ...current, model_compression_policies: currentPolicies }
      : current;
  });
}
