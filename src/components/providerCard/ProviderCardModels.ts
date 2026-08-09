import { connectionTestResults } from "../../features/providers/providerState";
import { renderConnectionTestState } from "../../features/providers/providerConnectionTests";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { UpstreamModel, VirtualModel } from "../../types/config";
import {
  reasoningLevelLabel,
  sortVirtualModelsByReasoningLevel,
} from "../../utils/reasoningUtils";

export interface ProviderModelLink {
  upstream: UpstreamModel;
  virtualModel: VirtualModel;
}

function capabilityBadge(type: "vision" | "tools" | "reasoning"): HTMLSpanElement {
  const icons = {
    vision: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
    tools: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`,
    reasoning: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  };
  const labels = {
    vision: t("models.vision"),
    tools: t("models.tools"),
    reasoning: t("models.reasoning"),
  };
  const badge = document.createElement("span");
  badge.className = "capability-badge";
  badge.title = labels[type];
  badge.innerHTML = `${icons[type]}${labels[type]}`;
  return badge;
}

function getCustomModelManagedStatus(upstreamId: string): { label: string; isManaged: boolean } {
  const settings = store.config.official_model_settings;
  if (!settings) return { label: t("models.officialStatusDirect"), isManaged: false };

  const override = settings.model_checkpoint_policies?.[upstreamId];
  if (override && override.enabled) {
    return { label: t("models.officialStatusManaged", { percent: "100" }), isManaged: true };
  }

  const custom = settings.custom_model;
  if (custom && custom.enabled) {
    const percent = custom.mode === "percentage"
      ? custom.token_threshold_percent
      : Math.round((custom.token_threshold / 1000000) * 100);
    return { label: t("models.officialStatusManaged", { percent: String(percent) }), isManaged: true };
  }

  return { label: t("models.officialStatusDirect"), isManaged: false };
}

function createModelGroup(upstream: UpstreamModel, virtualModels: VirtualModel[]): HTMLElement {
  const item = document.createElement("article");
  item.className = "provider-model-item";

  // 第一列：上游模型
  const main = document.createElement("div");
  main.className = "provider-model-main";
  const name = document.createElement("h4");
  name.textContent = upstream.display_name;
  main.append(name);

  // 第二列：能力 Badge
  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("vision"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("tools"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("reasoning"));
  }

  // 第三列：推理档位 (基于模型数据或默认)
  const reasoningCol = document.createElement("div");
  reasoningCol.className = "provider-model-variants-inline";
  
  // 第四列：压缩策略
  const policyCol = document.createElement("div");
  policyCol.className = "provider-model-variants-inline";

  for (const virtualModel of sortVirtualModelsByReasoningLevel(virtualModels)) {
    // 渲染推理档位 Pill
    const variant = document.createElement("div");
    variant.className = "model-variant-pill provider-model-variant";
    variant.dataset.virtualModelId = virtualModel.id;
    variant.title = virtualModel.display_name;

    const label = document.createElement("span");
    label.className = "model-variant-label";
    label.textContent = virtualModel.default_reasoning_level
      ? reasoningLevelLabel(virtualModel.default_reasoning_level)
      : t("models.defaultVariant");

    const result = document.createElement("span");
    result.className = "connection-result";
    result.setAttribute("role", "status");
    result.setAttribute("aria-live", "polite");
    result.hidden = true;
    const existingState = connectionTestResults.get(virtualModel.id);
    if (existingState) renderConnectionTestState(result, existingState);

    variant.append(label, result);
    reasoningCol.append(variant);
  }

  // 渲染压缩策略 (模型维度，仅需渲染一个)
  const policyPill = document.createElement("span");
  const status = getCustomModelManagedStatus(upstream.id);
  policyPill.className = status.isManaged ? "status-pill active" : "status-pill neutral";
  policyPill.textContent = status.label;
  policyCol.append(policyPill);

  item.append(main, capabilities, reasoningCol, policyCol);
  return item;
}

export function createProviderModels(
  upstreams: UpstreamModel[],
  modelLinks: ProviderModelLink[],
): HTMLDivElement {
  const models = document.createElement("div");
  models.className = "provider-models";
  if (modelLinks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = t("models.emptyTitle");
    models.append(empty);
    return models;
  }
  const header = document.createElement("div");
  header.className = "provider-models-header";
  for (const label of [
    t("models.upstreamModels"),
    t("models.capabilityColumn"),
    t("models.reasoningLevelColumn"),
    t("models.virtualModels"),
  ]) {
    const column = document.createElement("span");
    column.textContent = label;
    header.append(column);
  }
  models.append(header);

  const virtualsByUpstreamId = new Map<string, VirtualModel[]>();
  for (const { upstream, virtualModel } of modelLinks) {
    const virtualModels = virtualsByUpstreamId.get(upstream.id) ?? [];
    virtualModels.push(virtualModel);
    virtualsByUpstreamId.set(upstream.id, virtualModels);
  }

  for (const upstream of upstreams) {
    const virtualModels = virtualsByUpstreamId.get(upstream.id);
    if (!virtualModels || virtualModels.length === 0) continue;
    models.append(createModelGroup(upstream, virtualModels));
  }

  return models;
}
