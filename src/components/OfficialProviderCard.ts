import { updateConfig } from "../controllers/configController";
import { fetchOfficialModels } from "../controllers/providerController";
import { t } from "../i18n";
import { store } from "../store/appStore";
import type { ModelCompressionPolicy } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";
import { errorMessage } from "../utils/errorUtils";
import { reasoningLevelLabel } from "../utils/reasoningUtils";
import { showNotice } from "./NoticeBar";
import { getPolicyPillStatus, showPolicyEditorModal } from "./PolicyEditorModal";
import { buildModelCardUI } from "./providerCard/ModelCardUI";
import { createOfficialModelsDebugButtons } from "./OfficialModelsDebug";


function positiveMinimum(...values: Array<number | undefined>): number | null {
  const positiveValues = values.filter(
    (value): value is number => value !== undefined && Number.isFinite(value) && value > 0,
  );
  return positiveValues.length > 0 ? Math.min(...positiveValues) : null;
}

function capabilityBadge(type: "vision" | "tools" | "reasoning"): HTMLSpanElement {
  const icons = {
    vision: `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
    tools: `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`,
    reasoning: `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  };
  const labels = {
    vision: t("models.vision"),
    tools: t("models.tools"),
    reasoning: t("models.reasoning"),
  };
  const badge = document.createElement("span");
  badge.className = `capability-badge cap-${type}`;
  badge.title = labels[type];
  badge.setAttribute("aria-label", labels[type]);
  badge.innerHTML = icons[type];
  return badge;
}

let cachedOfficialModels: ProviderCatalogModel[] | null = null;
let cachedModelAliases: Map<string, string> = new Map();

function isOfficialSourceUnavailable(error: unknown): boolean {
  const code = error instanceof Error ? error.message : String(error);
  return code === "official_models_host_not_installed"
    || code === "official_models_host_not_running";
}

export function getCachedOfficialModelCount(): number | null {
  if (!cachedOfficialModels) return null;
  const mainModels = filterMainAgentModels(cachedOfficialModels);
  const disabledSet = new Set(store.config.disabled_official_models);
  return mainModels.filter((model) => !disabledSet.has(model.id)).length;
}

function buildOfficialModelCards(
  models: ProviderCatalogModel[],
  modelAliases: ReadonlyMap<string, string>,
  onToggle?: () => void,
): HTMLElement[] {
  const mainModels = filterMainAgentModels(models);
  if (!mainModels || mainModels.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = t("models.officialEmpty");
    return [empty];
  }

  // 极简单行 Pill Card 矩阵行：自动将带括号的相同前缀模型聚类
  interface GroupedOfficialModel {
    baseName: string;
    baseItem: ProviderCatalogModel;
    variants: { label: string; item: ProviderCatalogModel }[];
  }
  const groupMap = new Map<string, GroupedOfficialModel>();
  const groups: GroupedOfficialModel[] = [];

  for (const item of mainModels) {
    const displayName = item.displayName || item.id;
    const match = displayName.match(/^(.*?)(?:\s*\((.*?)\))?$/);
    const baseName = match?.[1] || displayName;

    let label = match?.[2];
    if (!label && item.reasoning?.levels && item.reasoning.levels.length > 0 && item.reasoning.levels[0] !== "off") {
      label = reasoningLevelLabel(item.reasoning.levels[0]);
    }
    if (!label) label = t("models.defaultVariant");

    let group = groupMap.get(baseName);
    if (!group) {
      group = { baseName, baseItem: item, variants: [] };
      groupMap.set(baseName, group);
      groups.push(group);
    }
    group.variants.push({ label, item });
  }

  const cards: HTMLElement[] = [];

  for (const group of groups) {
    const { baseName, baseItem: item, variants: groupVariants } = group;

    // 第一区：模型名称
    const name = document.createElement("h4");
    name.className = "model-card-title";
    name.textContent = baseName;

    const titleNode: HTMLElement = name;

    // 第二区：能力 Badge & 启用禁用 Toggle
    const capabilities = document.createElement("div");
    capabilities.className = "capability-list";
    const capObj = item.capabilities as Record<string, unknown> | undefined;
    const caps = capObj as Record<string, boolean> | undefined;
    if (caps?.vision) capabilities.append(capabilityBadge("vision"));
    if (caps?.tools) capabilities.append(capabilityBadge("tools"));
    if (caps?.reasoning) capabilities.append(capabilityBadge("reasoning"));

    const groupRelatedModelIds = new Set<string>();
    for (const variant of groupVariants) {
      for (const relatedId of officialRelatedModelIds(variant.item.id, modelAliases)) {
        groupRelatedModelIds.add(relatedId);
      }
    }
    const disabledSet = new Set(store.config.disabled_official_models);
    const isDisabled = Array.from(groupRelatedModelIds).some((id) => disabledSet.has(id));

    const toggleBtn = document.createElement("button");
    toggleBtn.type = "button";
    toggleBtn.className = `capability-badge action-badge model-toggle-btn ${isDisabled ? "disabled" : "enabled"}`;
    toggleBtn.title = isDisabled
      ? `${t("models.enableModel")} (${t("models.disabled")})`
      : `${t("models.disableModel")} (${t("models.enabled")})`;
    toggleBtn.setAttribute("aria-label", t(isDisabled ? "models.enableModel" : "models.disableModel"));
    toggleBtn.innerHTML = isDisabled
      ? `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>`
      : `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>`;

    toggleBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      void updateConfig((current) => {
        const nextDisabled = new Set(current.disabled_official_models);
        if (isDisabled) {
          for (const id of groupRelatedModelIds) nextDisabled.delete(id);
        } else {
          for (const id of groupRelatedModelIds) nextDisabled.add(id);
        }
        return {
          ...current,
          disabled_official_models: Array.from(nextDisabled),
        };
      }).then(() => {
        showNotice(t(isDisabled ? "models.modelEnabledNotice" : "models.modelDisabledNotice"));
        onToggle?.();
      });
    });

    capabilities.append(toggleBtn);

    // 第三区：变体 / 推理档位
    const variantsNode = document.createElement("div");
    variantsNode.className = "provider-model-variants-inline";
    for (const variant of groupVariants) {
      const pill = document.createElement("div");
      pill.className = "model-variant-pill provider-model-variant";
      const statusDot = document.createElement("span");
      statusDot.className = "connection-result success";
      const labelSpan = document.createElement("span");
      labelSpan.className = "model-variant-label";
      labelSpan.textContent = variant.label;
      pill.append(statusDot, labelSpan);
      variantsNode.append(pill);
    }

    // 第四区：压缩策略 (根据当前模型动态获取)
    const policyCol = document.createElement("div");
    policyCol.className = "provider-policy-col";
    const capacity = positiveMinimum(item.inputTokenLimit, item.maxTokens, item.contextWindow);
    const outputTokenLimit = positiveMinimum(item.outputTokenLimit);
    const policyModelId = canonicalOfficialModelId(item.id, modelAliases);
    const currentPolicy = store.config.model_compression_policies[policyModelId]
      ?? store.config.model_compression_policies[item.id]
      ?? null;
    const status = getPolicyPillStatus(
      currentPolicy,
      capacity,
      outputTokenLimit,
      t("models.presetOfficialDefault"),
    );

    const policyButton = document.createElement("button");
    policyButton.type = "button";
    policyButton.className = `policy-pill status-pill ${status.isManaged ? "accent" : "neutral"}`;
    policyButton.dataset.policyFocusKey = `official:${item.id}`;
    policyButton.title = status.tooltip;
    policyButton.setAttribute("aria-label", t("models.editPolicyForModel", {
      model: baseName,
      status: status.label,
    }));
    const policyLabel = document.createElement("span");
    policyLabel.textContent = status.label;
    policyButton.append(policyLabel);
    policyButton.insertAdjacentHTML("beforeend", `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>`);
    policyButton.addEventListener("click", () => {
      showPolicyEditorModal({
        scope: "official_threshold_override",
        modelName: baseName,
        currentPolicy,
        capacity,
        outputTokenLimit,
        defaultLabel: t("models.presetOfficialDefault"),
        defaultHelp: t("models.policyOfficialDefaultHelp"),
        emptyNotice: t("models.policyEmptyNotice"),
        upstreamCompression: item.upstreamCompression,
        focusKey: `official:${item.id}`,
        onSave: async (policy) => {
          await updateConfig((current) => {
            const policies = { ...current.model_compression_policies };
            for (const variant of groupVariants) {
              const relatedModelIds = officialRelatedModelIds(variant.item.id, modelAliases);
              for (const relatedModelId of relatedModelIds) {
                if (policy) policies[relatedModelId] = policy;
                else delete policies[relatedModelId];
              }
            }
            return { ...current, model_compression_policies: policies };
          });
        },
      });
    });
    policyCol.append(policyButton);

    const card = buildModelCardUI({
      titleNode,
      capabilitiesNode: capabilities,
      variantsNode,
      policyNode: policyCol,
    });
    if (isDisabled) {
      card.classList.add("disabled-official-card");
    }

    cards.push(card);
  }

  return cards;
}

export function renderOfficialProviderCard(options: {
  onModelCountChange?: (count: number | null) => void;
} = {}): { element: HTMLElement; dispose: () => void } {
  const card = document.createElement("article");
  card.className = "provider-card";

  // Card Top Toolbar (测试状态提示 + 操作按钮)
  const toolbar = document.createElement("div");
  toolbar.className = "provider-card-toolbar";

  const toolbarLeft = document.createElement("div");
  toolbarLeft.className = "provider-toolbar-left";

  const summary = document.createElement("span");
  summary.className = "provider-test-summary";
  toolbarLeft.append(summary);

  const toolbarRight = document.createElement("div");
  toolbarRight.className = "provider-toolbar-right";

  const testBtn = document.createElement("button");
  testBtn.type = "button";
  testBtn.className = "secondary compact-button";
  testBtn.textContent = t("models.testConnection");

  testBtn.addEventListener("click", () => {
    testBtn.disabled = true;
    testBtn.textContent = t("models.testing");
    const startTime = performance.now();
    void fetchOfficialModels()
      .then(() => {
        const duration = Math.round(performance.now() - startTime);
        summary.className = "provider-test-summary success";
        summary.textContent = t("models.testSuccess", { time: String(duration) });
      })
      .catch((err: unknown) => {
        summary.className = "provider-test-summary error";
        summary.textContent = t("models.testFailed", { msg: errorMessage(err) });
      })
      .finally(() => {
        testBtn.disabled = false;
        testBtn.textContent = t("models.testConnection");
      });
  });

  const refreshBtn = document.createElement("button");
  refreshBtn.type = "button";
  refreshBtn.className = "secondary compact-button";
  refreshBtn.textContent = t("overview.refresh");
  refreshBtn.addEventListener("click", () => {
    refreshBtn.disabled = true;
    summary.className = "provider-test-summary";
    summary.textContent = "";
    loadOfficialModels(true).finally(() => {
      refreshBtn.disabled = false;
    });
  });

  toolbarRight.append(testBtn, refreshBtn);
  toolbarRight.append(...createOfficialModelsDebugButtons());
  toolbar.append(toolbarLeft, toolbarRight);
  card.append(toolbar);

  // Table Wrapper & Models 列表
  const tableWrapper = document.createElement("div");
  tableWrapper.className = "provider-table-wrapper";

  const modelsContainer = document.createElement("div");
  modelsContainer.className = "provider-models";

  const refreshCardList = () => {
    if (!cachedOfficialModels) return;
    options.onModelCountChange?.(getCachedOfficialModelCount());
    modelsContainer.replaceChildren(
      ...buildOfficialModelCards(cachedOfficialModels, cachedModelAliases, refreshCardList),
    );
  };

  // 1. 若已有缓存数据：立即同步渲染已有模型，杜绝空白与 Loading 闪烁
  if (cachedOfficialModels && cachedOfficialModels.length > 0) {
    refreshCardList();
  } else {
    // 首次进入无缓存时展示 Loading 提示
    const loadingState = document.createElement("p");
    loadingState.className = "provider-model-empty";
    loadingState.textContent = t("models.officialFetching");
    modelsContainer.append(loadingState);
  }

  tableWrapper.append(modelsContainer);
  card.append(tableWrapper);

  let isDisposed = false;

  const loadOfficialModels = (isManualRefresh = false) => {
    if (!cachedOfficialModels && !isManualRefresh) {
      options.onModelCountChange?.(null);
    }
    return fetchOfficialModels()
      .then((models) => {
        if (isDisposed) return;
        cachedOfficialModels = models;
        cachedModelAliases = officialModelAliases(models);

        void synchronizeOfficialModelPolicies(cachedModelAliases).catch((error: unknown) => {
          console.warn("同步官方模型策略失败，代理运行时仍会按目录映射兼容", error);
        });
        options.onModelCountChange?.(getCachedOfficialModelCount());
        summary.className = "provider-test-summary";
        summary.textContent = "";

        // 平滑就地替换卡片
        modelsContainer.replaceChildren(
          ...buildOfficialModelCards(models, cachedModelAliases, refreshCardList),
        );
      })
      .catch((err: unknown) => {
        if (isDisposed) return;
        const message = errorMessage(err);
        summary.className = "provider-test-summary error";
        summary.textContent = t("models.officialStatusFailed");

        if (isOfficialSourceUnavailable(err)) {
          cachedOfficialModels = null;
          cachedModelAliases = new Map();
        }
        if (!cachedOfficialModels || cachedOfficialModels.length === 0) {
          options.onModelCountChange?.(null);
          modelsContainer.replaceChildren();
          const errorMsg = document.createElement("p");
          errorMsg.className = "provider-model-empty";
          errorMsg.textContent = t("models.officialFetchFailed", { message });
          modelsContainer.append(errorMsg);
        }
      });
  };

  // 后台静默刷新（若已有缓存则无感知校验，若无缓存则拉取初始数据）
  void loadOfficialModels(false);

  return {
    element: card,
    dispose: () => {
      isDisposed = true;
    },
  };
}

function filterMainAgentModels(models: ProviderCatalogModel[]): ProviderCatalogModel[] {
  const hasAgentMetadata = models.some((model) => model.isAgentModel !== undefined);
  const hasRecommendationMetadata = models.some((model) => model.isRecommended !== undefined);
  const filtered = hasAgentMetadata
    ? models.filter(
      (model) =>
        model.isAgentModel === true
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

function officialModelAliases(models: ProviderCatalogModel[]): Map<string, string> {
  const aliases = new Map<string, string>();
  for (const model of models) {
    if (model.isDeprecated && model.replacementModelId) {
      aliases.set(model.id, model.replacementModelId);
    }
  }
  return aliases;
}

function canonicalOfficialModelId(modelId: string, aliases: ReadonlyMap<string, string>): string {
  let canonicalId = modelId;
  const visited = new Set<string>();
  while (aliases.has(canonicalId) && !visited.has(canonicalId)) {
    visited.add(canonicalId);
    canonicalId = aliases.get(canonicalId) ?? canonicalId;
  }
  return canonicalId;
}

function officialRelatedModelIds(
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

async function synchronizeOfficialModelPolicies(
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

function synchronizedOfficialPolicies(
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
