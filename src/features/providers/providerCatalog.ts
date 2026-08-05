import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelTokenLimits, Provider, ProviderProtocol } from "../../types/config";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import { store } from "../../store/appStore";
import { fetchProviderCatalog as fetchProviderCatalogCommand } from "../../controllers/providerController";
import { element } from "../../utils/domUtils";
import {
  catalogReasoningLevelsForModel,
  catalogReasoningMetadataLabel,
  catalogReasoningIsAuthoritative,
  customReasoningValueFromUpstream,
  reasoningLevelLabel,
  reasoningLevelsForVirtualModels,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import { openReasoningModal } from "../../components/ReasoningModal";
import { t } from "../../i18n";
import { runCatalogModelTests, testProviderModelConnection } from "./providerTesting";
import {
  formatTokenLimit,
  presetIdForTokenLimits,
  resolveCatalogTokenLimits,
  TOKEN_LIMIT_PRESETS,
  tokenLimitsForPreset,
} from "./tokenLimits";

export let catalogModels: ProviderCatalogModel[] = [];
export let selectedCatalogModelIds = new Set<string>();
export let catalogReasoningLevelsByModel = new Map<string, Set<ConfigurableReasoningLevel>>();
export let catalogCustomReasoningByModel = new Map<string, string>();
export let catalogVisionEnabledModelIds = new Set<string>();
export let catalogToolsEnabledModelIds = new Set<string>();
export let catalogReasoningEnabledModelIds = new Set<string>();
export let catalogTokenLimitsByModel = new Map<string, ModelTokenLimits>();
export let changedCatalogTokenLimitModelIds = new Set<string>();
export let changedCatalogCapabilityModelIds = new Set<string>();
export let changedCatalogReasoningModelIds = new Set<string>();
export let legacyCatalogModelIds = new Set<string>();
let catalogFetchedCount = 0;
let catalogStatusHasLegacy = false;

export interface ProviderCatalogState {
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogVisionEnabledModelIds: ReadonlySet<string>;
  catalogToolsEnabledModelIds: ReadonlySet<string>;
  catalogReasoningEnabledModelIds: ReadonlySet<string>;
  catalogTokenLimitsByModel: ReadonlyMap<string, ModelTokenLimits>;
  changedCatalogTokenLimitModelIds: ReadonlySet<string>;
  changedCatalogCapabilityModelIds: ReadonlySet<string>;
  changedCatalogReasoningModelIds: ReadonlySet<string>;
  legacyCatalogModelIds: ReadonlySet<string>;
}

export function getProviderCatalogState(): ProviderCatalogState {
  return {
    catalogModels,
    selectedCatalogModelIds,
    catalogReasoningLevelsByModel,
    catalogCustomReasoningByModel,
    catalogVisionEnabledModelIds,
    catalogToolsEnabledModelIds,
    catalogReasoningEnabledModelIds,
    catalogTokenLimitsByModel,
    changedCatalogTokenLimitModelIds,
    changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds,
    legacyCatalogModelIds,
  };
}

export interface ProviderCatalogContext {
  getEditingProviderId: () => string | null;
  selectedProtocol: () => ProviderProtocol;
  providerFromForm: () => Provider;
  setProviderEditorDirty: (dirty: boolean) => void;
  withProviderEditorBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
  invalidatePendingProviderSave: () => void;
  refreshProviderEditorControls: () => void;
}

export function renderCatalogStatus(): void {
  const status = element<HTMLElement>("#catalog-status");
  status.textContent = catalogFetchedCount === 0
    ? t("models.fetching")
    : catalogStatusHasLegacy
      ? t("models.catalogFetchedWithLegacy", {
          count: catalogFetchedCount,
          legacy: legacyCatalogModelIds.size,
        })
      : t("models.catalogFetched", { count: catalogFetchedCount });
}

export function resetCatalogResults(): void {
  catalogFetchedCount = 0;
  catalogStatusHasLegacy = false;
  catalogModels = [];
  selectedCatalogModelIds = new Set();
  catalogReasoningLevelsByModel = new Map();
  catalogCustomReasoningByModel = new Map();
  catalogVisionEnabledModelIds = new Set();
  catalogToolsEnabledModelIds = new Set();
  catalogReasoningEnabledModelIds = new Set();
  catalogTokenLimitsByModel = new Map();
  changedCatalogTokenLimitModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();
  element<HTMLElement>("#catalog-results").hidden = true;
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  element<HTMLButtonElement>("#save-provider").disabled = true;
  renderCatalogStatus();
}

export async function fetchProviderCatalog(context: ProviderCatalogContext): Promise<void> {
  const providerForm = element<HTMLFormElement>("#provider-form");
  if (!providerForm.reportValidity()) return;
  context.invalidatePendingProviderSave();
  context.refreshProviderEditorControls();
  const provider = context.providerFromForm();
  const fetched = await fetchProviderCatalogCommand(provider);
  const fetchedIds = new Set(fetched.map((model) => model.id));
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const editingProviderId = context.getEditingProviderId();
  const existingUpstreams = editingProviderId
    ? store.config.upstream_models.filter((item) => item.provider_id === editingProviderId)
    : [];
  legacyCatalogModelIds = new Set(
    existingUpstreams
      .filter((upstream) => !fetchedIds.has(upstream.upstream_model_id))
      .map((upstream) => upstream.upstream_model_id),
  );
  for (const upstream of existingUpstreams) {
    if (!byId.has(upstream.upstream_model_id)) {
      byId.set(upstream.upstream_model_id, {
        id: upstream.upstream_model_id,
        displayName: upstream.display_name,
      });
    }
  }
  catalogModels = [...byId.values()];
  selectedCatalogModelIds = new Set(
    existingUpstreams.map((item) => item.upstream_model_id),
  );
  changedCatalogTokenLimitModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  const existingUpstreamsByModelId = new Map(
    existingUpstreams.map((upstream) => [upstream.upstream_model_id, upstream]),
  );
  catalogTokenLimitsByModel = new Map(
    catalogModels.map((model) => [
      model.id,
      resolveCatalogTokenLimits(model, existingUpstreamsByModelId.get(model.id)?.token_limits),
    ]),
  );
  catalogVisionEnabledModelIds = new Set(
    catalogModels
      .filter((model) => existingUpstreamsByModelId.get(model.id)?.capabilities.vision ?? true)
      .map((model) => model.id),
  );
  catalogToolsEnabledModelIds = new Set(
    catalogModels
      .filter((model) => existingUpstreamsByModelId.get(model.id)?.capabilities.tools ?? true)
      .map((model) => model.id),
  );
  catalogReasoningEnabledModelIds = new Set(
    catalogModels
      .filter((model) => {
        const upstream = existingUpstreamsByModelId.get(model.id);
        const hasConcreteCatalogReasoning = catalogReasoningIsAuthoritative(model);
        if (!upstream) return hasConcreteCatalogReasoning;
        return Object.keys(upstream.capabilities.reasoning.levels).length > 0;
      })
      .map((model) => model.id),
  );
  catalogReasoningLevelsByModel = new Map(catalogModels.map((model) => {
    const upstream = existingUpstreamsByModelId.get(model.id);
    const catalogLevels = catalogReasoningLevelsForModel(model, provider.protocol, upstream);
    const hasConcreteCatalogReasoning = catalogReasoningIsAuthoritative(model);
    if (!upstream) {
      return [model.id, new Set(hasConcreteCatalogReasoning ? catalogLevels : [])];
    }
    const virtualModels = store.config.virtual_models.filter(
      (item) => item.upstream_model_id === upstream.id,
    );
    const existingLevels = reasoningLevelsForVirtualModels(provider.protocol, virtualModels);
    return [model.id, new Set(existingLevels)];
  }));
  catalogCustomReasoningByModel = new Map(
    catalogModels.flatMap((model) => {
      const upstream = existingUpstreamsByModelId.get(model.id);
      const value = upstream ? customReasoningValueFromUpstream(upstream) : null;
      return value ? [[model.id, value] as const] : [];
    }),
  );
  catalogFetchedCount = fetched.length;
  catalogStatusHasLegacy = legacyCatalogModelIds.size > 0;
  element<HTMLElement>("#catalog-results").hidden = false;
  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>("#catalog-results").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

export function updateCatalogSelection(context: ProviderCatalogContext): void {
  const count = selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent = count > 0
    ? t("models.selectedModels", { count })
    : t("models.noModelSelected");
  context.refreshProviderEditorControls();
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleIds = catalogModels
    .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
    .map((model) => model.id);
  const selectAll = element<HTMLInputElement>("#select-all-models");
  selectAll.checked = visibleIds.length > 0
    && visibleIds.every((id) => selectedCatalogModelIds.has(id));
  selectAll.indeterminate = visibleIds.some((id) => selectedCatalogModelIds.has(id))
    && !selectAll.checked;
}

function catalogCapabilityToggle(
  modelId: string,
  label: string,
  enabledModelIds: Set<string>,
  onChange: () => void,
): HTMLLabelElement {
  const toggle = document.createElement("label");
  toggle.className = "check-label catalog-capability-toggle";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = enabledModelIds.has(modelId);
  checkbox.addEventListener("change", () => {
    if (checkbox.checked) enabledModelIds.add(modelId);
    else enabledModelIds.delete(modelId);
    onChange();
  });
  const copy = document.createElement("span");
  copy.textContent = label;
  toggle.append(checkbox, copy);
  return toggle;
}

function tokenPresetName(id: string): string {
  const labels: Record<string, string> = {
    catalog: t("models.tokenPresetCatalog"),
    chatgpt_default: t("models.tokenPresetChatgptDefault"),
    chatgpt_thinking: t("models.tokenPresetChatgptThinking"),
    gpt5_api: t("models.tokenPresetGpt5Api"),
    gemini_long: t("models.tokenPresetGeminiLong"),
    claude_long: t("models.tokenPresetClaudeLong"),
    compatibility: t("models.tokenPresetCompatibility"),
    custom: t("models.tokenPresetCustom"),
  };
  return labels[id] ?? id;
}

function createTokenLimitControls(
  model: ProviderCatalogModel,
  selected: boolean,
  context: ProviderCatalogContext,
): HTMLDivElement {
  const control = document.createElement("div");
  control.className = "catalog-token-controls";

  const currentLimits = catalogTokenLimitsByModel.get(model.id)
    ?? resolveCatalogTokenLimits(model);
  const hasCatalogInput = model.inputTokenLimit !== undefined;
  const hasCatalogOutput = model.outputTokenLimit !== undefined;
  const titleRow = document.createElement("div");
  titleRow.className = "catalog-token-heading";
  const title = document.createElement("span");
  title.className = "catalog-token-title";
  title.textContent = t("models.tokenLimitTitle");
  const sourceNote = document.createElement("span");
  sourceNote.className = `catalog-token-source-note${hasCatalogInput || hasCatalogOutput ? " reported" : " missing"}`;
  sourceNote.textContent = hasCatalogInput || hasCatalogOutput
    ? t("models.tokenLimitCatalogValue")
    : t("models.tokenLimitMissing");
  titleRow.append(title, sourceNote);
  const updateSummary = (summary: HTMLElement) => {
    const displayedLimits = catalogTokenLimitsByModel.get(model.id) ?? currentLimits;
    summary.textContent = t("models.tokenLimitSummary", {
      input: formatTokenLimit(displayedLimits.input_token_limit),
      output: formatTokenLimit(displayedLimits.output_token_limit),
    });
  };
  const updateManualLimit = (
    field: "input_token_limit" | "output_token_limit",
    value: string,
    summary: HTMLElement,
  ) => {
    const trimmed = value.trim();
    const parsed = trimmed.length === 0 ? null : Number(trimmed);
    if (parsed !== null && (!Number.isInteger(parsed) || parsed <= 0 || parsed > 0x7fffffff)) {
      return;
    }
    const next = {
      ...(catalogTokenLimitsByModel.get(model.id) ?? currentLimits),
      [field]: parsed,
    };
    catalogTokenLimitsByModel.set(model.id, next);
    changedCatalogTokenLimitModelIds.add(model.id);
    updateSummary(summary);
    context.setProviderEditorDirty(true);
    context.refreshProviderEditorControls();
  };

  const fields = document.createElement("div");
  fields.className = "catalog-token-fields";
  const summary = document.createElement("span");
  summary.className = "catalog-token-summary";
  updateSummary(summary);
  const contextSummary = document.createElement("span");
  contextSummary.className = "catalog-token-context";
  const contextLimit = model.contextWindow ?? model.contextLength;
  const contextParts: string[] = [];
  if (contextLimit !== undefined) {
    contextParts.push(t("models.tokenContextSummary", {
      context: formatTokenLimit(contextLimit),
    }));
  }
  if (model.maxContextWindow !== undefined && model.maxContextWindow !== contextLimit) {
    contextParts.push(t("models.tokenNativeContextSummary", {
      context: formatTokenLimit(model.maxContextWindow),
    }));
  }
  if (model.autoCompactTokenLimit !== undefined) {
    contextParts.push(t("models.tokenAutoCompactSummary", {
      context: formatTokenLimit(model.autoCompactTokenLimit),
    }));
  } else if (contextLimit !== undefined) {
    contextParts.push(t("models.tokenAutoCompactMissing"));
  }
  if (contextParts.length > 0) {
    contextSummary.textContent = contextParts.join(" · ");
  }

  const appendField = (
    field: "input_token_limit" | "output_token_limit",
    reportedValue: number | undefined,
    labelKey: "tokenInputLimit" | "tokenOutputLimit",
  ) => {
    const fieldRow = document.createElement("label");
    fieldRow.className = "catalog-token-field";
    const fieldLabel = document.createElement("span");
    fieldLabel.textContent = t(`models.${labelKey}`);
    fieldRow.append(fieldLabel);
    if (reportedValue !== undefined) {
      const value = document.createElement("span");
      value.className = "catalog-token-value readonly";
      value.textContent = formatTokenLimit(reportedValue);
      value.title = t("models.tokenLimitCatalogValue");
      const source = document.createElement("span");
      source.className = "catalog-token-source";
      source.textContent = t("models.tokenLimitCatalogValue");
      fieldRow.append(value, source);
    } else {
      const input = document.createElement("input");
      input.type = "number";
      input.min = "1";
      input.step = "1";
      input.className = "catalog-token-input";
      const displayedValue = catalogTokenLimitsByModel.get(model.id)?.[field] ?? currentLimits[field];
      input.value = displayedValue === null ? "" : String(displayedValue);
      input.disabled = !selected;
      input.placeholder = t("models.tokenLimitManualValue");
      input.title = t("models.tokenLimitManualValue");
      input.addEventListener("change", () => updateManualLimit(field, input.value, summary));
      const source = document.createElement("span");
      source.className = "catalog-token-source manual";
      source.textContent = t("models.tokenLimitManualValue");
      fieldRow.append(input, source);
    }
    fields.append(fieldRow);
  };

  appendField("input_token_limit", model.inputTokenLimit, "tokenInputLimit");
  appendField("output_token_limit", model.outputTokenLimit, "tokenOutputLimit");

  if (!hasCatalogInput && !hasCatalogOutput) {
    const preset = document.createElement("select");
    preset.className = "catalog-token-preset";
    for (const item of TOKEN_LIMIT_PRESETS) {
      const option = document.createElement("option");
      option.value = item.id;
      option.textContent = tokenPresetName(item.id);
      preset.append(option);
    }
    const currentPreset = presetIdForTokenLimits(currentLimits);
    if (currentPreset === "custom") {
      const customOption = document.createElement("option");
      customOption.value = "custom";
      customOption.textContent = tokenPresetName("custom");
      customOption.disabled = true;
      preset.append(customOption);
    }
    preset.value = currentPreset;
    preset.disabled = !selected;
    preset.title = t("models.tokenLimitPresetHint");
    preset.addEventListener("change", () => {
      const nextLimits = tokenLimitsForPreset(preset.value);
      if (!nextLimits) return;
      catalogTokenLimitsByModel.set(model.id, nextLimits);
      changedCatalogTokenLimitModelIds.add(model.id);
      updateSummary(summary);
      fields.querySelectorAll<HTMLInputElement>(".catalog-token-input").forEach((input, index) => {
        const field = index === 0 ? "input_token_limit" : "output_token_limit";
        const value = nextLimits[field];
        input.value = value === null ? "" : String(value);
      });
      context.setProviderEditorDirty(true);
      context.refreshProviderEditorControls();
    });
    control.append(titleRow, preset, fields, summary, contextSummary);
  } else {
    control.append(titleRow, fields, summary, contextSummary);
  }
  return control;
}

export function renderCatalogModels(context: ProviderCatalogContext): void {
  const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    const selected = selectedCatalogModelIds.has(model.id);
    const editingProviderId = context.getEditingProviderId();
    const existingUpstream = editingProviderId
      ? store.config.upstream_models.find(
          (item) => item.provider_id === editingProviderId && item.upstream_model_id === model.id,
        )
      : undefined;
    row.className = `catalog-model-row${selected ? "" : " unselected"}${legacyCatalogModelIds.has(model.id) ? " legacy" : ""}`;
    const select = document.createElement("label");
    select.className = "catalog-model-select";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selected;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedCatalogModelIds.add(model.id);
      else selectedCatalogModelIds.delete(model.id);
      context.setProviderEditorDirty(true);
      renderCatalogModels(context);
    });
    const copy = document.createElement("span");
    const name = document.createElement("strong");
    name.textContent = model.displayName;
    const id = document.createElement("code");
    id.textContent = model.id;
    copy.append(name);
    if (legacyCatalogModelIds.has(model.id)) {
      const legacy = document.createElement("span");
      legacy.className = "legacy-badge";
      legacy.textContent = t("models.currentCatalogMissing");
      legacy.title = t("models.currentCatalogMissingHint");
      copy.append(legacy);
    }
    copy.append(id);
    const reasoningMetadataLabel = catalogReasoningMetadataLabel(model);
    if (reasoningMetadataLabel) {
      const reasoningHint = document.createElement("span");
      reasoningHint.className = `catalog-reasoning-hint${model.reasoning?.supported === false ? " unsupported" : ""}`;
      reasoningHint.textContent = reasoningMetadataLabel;
      copy.append(reasoningHint);
    }
    select.append(checkbox, copy);

    const capabilities = document.createElement("div");
    capabilities.className = "catalog-model-capabilities";
    const selectedLevels = catalogReasoningLevelsByModel.get(model.id);
    const availableReasoningLevels = catalogReasoningLevelsForModel(
      model,
      context.selectedProtocol(),
      existingUpstream,
    );
    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id) && (selectedLevels?.size ?? 0) > 0;
    const reasoningBtn = document.createElement("button");
    reasoningBtn.type = "button";
    reasoningBtn.className = `catalog-reasoning-trigger${reasoningEnabled ? " active" : ""}`;
    const reasoningLevelsSummary = reasoningEnabled
      ? sortReasoningLevels(selectedLevels!).map(reasoningLevelLabel).join(" · ")
      : "";
    reasoningBtn.textContent = reasoningEnabled
      ? t("models.reasoningSummary", { levels: reasoningLevelsSummary })
      : t("models.configureReasoning");
    const reasoningToggleLabel = catalogReasoningMetadataLabel(model);
    reasoningBtn.title = reasoningToggleLabel ?? t("models.configureReasoningHint");
    reasoningBtn.disabled = !selected || availableReasoningLevels.length === 0;
    reasoningBtn.addEventListener("click", () => {
      openReasoningModal(model, {
        providerProtocol: context.selectedProtocol(),
        existingUpstream,
        currentLevels: selectedLevels ?? new Set<ConfigurableReasoningLevel>(),
        providerFromForm: context.providerFromForm,
        testProviderModelConnection,
        runBusy: context.withProviderEditorBusy,
        onConfirm: (modelId, levels) => {
          const previousLevels = catalogReasoningLevelsByModel.get(modelId)
            ?? new Set<ConfigurableReasoningLevel>();
          const levelsChanged = previousLevels.size !== levels.size
            || [...previousLevels].some((level) => !levels.has(level));
          if (levels.size > 0) {
            catalogReasoningEnabledModelIds.add(modelId);
            catalogReasoningLevelsByModel.set(modelId, levels);
          } else {
            catalogReasoningEnabledModelIds.delete(modelId);
            catalogReasoningLevelsByModel.delete(modelId);
          }
          if (levelsChanged) {
            changedCatalogReasoningModelIds.add(modelId);
            context.setProviderEditorDirty(true);
          }
          renderCatalogModels(context);
        },
      });
    });

    capabilities.append(
      catalogCapabilityToggle(model.id, t("models.visionInput"), catalogVisionEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        context.setProviderEditorDirty(true);
      }),
      catalogCapabilityToggle(model.id, t("models.toolCalling"), catalogToolsEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        context.setProviderEditorDirty(true);
      }),
      reasoningBtn,
    );
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = !selected;
    }

    const test = document.createElement("button");
    test.type = "button";
    test.className = "secondary compact-button";
    test.textContent = t("models.testConnectionShort");
    test.title = t("models.testSelectedReasoning");
    const result = document.createElement("span");
    result.className = "catalog-model-test-result";
    result.setAttribute("role", "status");
    test.addEventListener("click", () => {
      runCatalogModelTests({
        button: test,
        result,
        modelId: model.id,
        model,
        existingUpstream,
        providerFromForm: context.providerFromForm,
        isReasoningEnabled: () => catalogReasoningEnabledModelIds.has(model.id),
        selectedReasoningLevels: () => catalogReasoningLevelsByModel.get(model.id) ?? new Set<ConfigurableReasoningLevel>(),
        runBusy: context.withProviderEditorBusy,
      });
    });
    const testArea = document.createElement("div");
    testArea.className = "catalog-model-test-area";
    testArea.append(test, result);
    const header = document.createElement("div");
    header.className = "catalog-model-header";
    header.append(select, testArea);

    const capabilityGroup = document.createElement("div");
    capabilityGroup.className = "catalog-capability-group";
    const capabilityTitle = document.createElement("span");
    capabilityTitle.className = "catalog-capability-title";
    capabilityTitle.textContent = t("models.capabilityColumn");
    capabilityGroup.append(capabilityTitle, capabilities);

    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.append(createTokenLimitControls(model, selected, context));
    actions.append(capabilityGroup);
    row.append(header, actions);
    catalogModelList.append(row);
  }

  if (visibleModels.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state compact-empty";
    empty.textContent = t("models.noMatchingModels");
    catalogModelList.append(empty);
  }
  updateCatalogSelection(context);
}
