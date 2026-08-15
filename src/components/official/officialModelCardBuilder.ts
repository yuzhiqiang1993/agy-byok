import { updateConfig } from "../../controllers/configController";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import { reasoningLevelLabel } from "../../utils/reasoningUtils";
import { showNotice } from "../NoticeBar";
import { getPolicyPillStatus, showPolicyEditorModal } from "../PolicyEditorModal";
import { buildModelCardUI } from "../providerCard/ModelCardUI";
import {
  canonicalOfficialModelId,
  filterMainAgentModels,
  officialRelatedModelIds,
} from "./officialModelUtils";

function positiveMinimum(...values: Array<number | undefined>): number | null {
  const positiveValues = values.filter(
    (value): value is number => value !== undefined && Number.isFinite(value) && value > 0,
  );
  return positiveValues.length > 0 ? Math.min(...positiveValues) : null;
}

export function capabilityBadge(type: "vision" | "tools" | "reasoning"): HTMLSpanElement {
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

export interface GroupedOfficialModel {
  baseName: string;
  baseItem: ProviderCatalogModel;
  variants: { label: string; item: ProviderCatalogModel }[];
}

export function buildOfficialModelCards(
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
    if (item.inputModalities?.includes("image")) capabilities.append(capabilityBadge("vision"));
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
