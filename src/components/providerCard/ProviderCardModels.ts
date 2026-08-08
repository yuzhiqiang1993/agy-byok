import { connectionTestResults } from "../../features/providers/providerState";
import { renderConnectionTestState } from "../../features/providers/providerConnectionTests";
import { t } from "../../i18n";
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

function createModelGroup(upstream: UpstreamModel, virtualModels: VirtualModel[]): HTMLElement {
  const item = document.createElement("article");
  item.className = "provider-model-item";
  const main = document.createElement("div");
  main.className = "provider-model-main";
  const name = document.createElement("h4");
  name.textContent = upstream.display_name;
  main.append(name);
  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("vision"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("tools"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("reasoning"));
  }

  const variants = document.createElement("div");
  variants.className = "provider-model-variants-inline";
  for (const virtualModel of sortVirtualModelsByReasoningLevel(virtualModels)) {
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
    variants.append(variant);
  }
  item.append(main, capabilities, variants);
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
  for (const label of [t("models.upstreamModels"), t("models.capabilityColumn"), t("models.virtualModels")]) {
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
    const virtualModels = virtualsByUpstreamId.get(upstream.id) ?? [];
    if (virtualModels.length > 0) models.append(createModelGroup(upstream, virtualModels));
  }
  return models;
}
