import { updateConfig } from "../../controllers/configController";
import { renderConnectionTestState } from "../../features/providers/providerConnectionTests";
import { connectionTestResults } from "../../features/providers/providerState";
import { t } from "../../i18n";
import type { UpstreamModel, VirtualModel } from "../../types/config";
import { getPolicyPillStatus, showPolicyEditorModal } from "../PolicyEditorModal";
import {
  reasoningLevelLabel,
  sortVirtualModelsByReasoningLevel,
} from "../../utils/reasoningUtils";
import { buildModelCardUI } from "./ModelCardUI";

export interface ProviderModelLink {
  upstream: UpstreamModel;
  virtualModel: VirtualModel;
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

function positiveMinimum(...values: Array<number | null>): number | null {
  const positiveValues = values.filter(
    (value): value is number => value !== null && Number.isFinite(value) && value > 0,
  );
  return positiveValues.length > 0 ? Math.min(...positiveValues) : null;
}

function createModelGroup(upstream: UpstreamModel, virtualModels: VirtualModel[]): HTMLElement {
  // --- Header ---
  const name = document.createElement("h4");
  name.className = "model-card-title";
  name.textContent = upstream.display_name;

  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("vision"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("tools"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("reasoning"));
  }

  // --- Body ---
  const reasoningCol = document.createElement("div");
  reasoningCol.className = "provider-model-variants-inline";
  
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
    reasoningCol.append(variant);
  }

  // --- Footer ---
  const policyCol = document.createElement("div");
  policyCol.className = "provider-policy-col";

  const capacity = positiveMinimum(
    upstream.token_limits.context_window,
    upstream.token_limits.input_token_limit,
  );
  const outputTokenLimit = upstream.token_limits.output_token_limit_source === "estimated"
    ? null
    : positiveMinimum(upstream.token_limits.output_token_limit);
  const status = getPolicyPillStatus(
    upstream.compression_policy,
    capacity,
    outputTokenLimit,
    t("models.presetUpstreamDefault"),
  );
  const policyButton = document.createElement("button");
  policyButton.type = "button";
  policyButton.className = `policy-pill status-pill ${status.isManaged ? "accent" : "neutral"}`;
  policyButton.dataset.policyFocusKey = `upstream:${upstream.id}`;
  policyButton.title = status.tooltip;
  policyButton.setAttribute("aria-label", t("models.editPolicyForModel", {
    model: upstream.display_name || upstream.upstream_model_id,
    status: status.label,
  }));
  const policyLabel = document.createElement("span");
  policyLabel.textContent = status.label;
  policyButton.append(policyLabel);
  policyButton.insertAdjacentHTML("beforeend", `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>`);
  policyButton.addEventListener("click", () => {
    showPolicyEditorModal({
      modelName: upstream.display_name || upstream.upstream_model_id,
      currentPolicy: upstream.compression_policy,
      capacity,
      outputTokenLimit,
      defaultLabel: t("models.presetUpstreamDefault"),
      defaultHelp: t("models.policyCustomUnconfiguredHelp"),
      emptyNotice: t("models.policyEmptyNoticeCustom"),
      preferCurrentWorker: true,
      focusKey: `upstream:${upstream.id}`,
      onSave: async (policy) => {
        await updateConfig((current) => ({
          ...current,
          upstream_models: current.upstream_models.map((item) => (
            item.id === upstream.id ? { ...item, compression_policy: policy } : item
          )),
        }));
      },
    });
  });

  policyCol.append(policyButton);
  
  return buildModelCardUI({
    titleNode: name,
    capabilitiesNode: capabilities,
    variantsNode: reasoningCol,
    policyNode: policyCol,
  });
}

export function createProviderModels(
  upstreams: UpstreamModel[],
  modelLinks: ProviderModelLink[],
): HTMLDivElement {
  const wrapper = document.createElement("div");
  wrapper.className = "provider-table-wrapper";

  const models = document.createElement("div");
  models.className = "provider-models";
  if (modelLinks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = t("models.emptyTitle");
    models.append(empty);
    wrapper.append(models);
    return wrapper;
  }

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

  wrapper.append(models);
  return wrapper;
}
