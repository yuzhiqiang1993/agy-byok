import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  ModelCheckpointOverride,
  ModelTokenLimits,
  Provider,
  ProviderProtocol,
  TokenLimitSource,
} from "../../types/config";
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
  catalogContextWindow,
  customModelCheckpointLimits,
  CONTEXT_WINDOW_OPTIONS,
  DEFAULT_CONTEXT_WINDOW,
  DEFAULT_TOKEN_LIMIT,
  formatTokenLimit,
  isValidModelCheckpointOverride,
  presetIdForTokenLimits,
  resolveCatalogTokenLimits,
  TOKEN_INPUT_LIMIT_OPTIONS,
  TOKEN_LIMIT_PRESETS,
  TOKEN_OUTPUT_LIMIT_OPTIONS,
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
export let catalogCheckpointOverridesByModel = new Map<string, ModelCheckpointOverride | null>();
export let changedCatalogTokenLimitModelIds = new Set<string>();
export let changedCatalogCheckpointOverrideModelIds = new Set<string>();
export let changedCatalogCapabilityModelIds = new Set<string>();
export let changedCatalogReasoningModelIds = new Set<string>();
export let legacyCatalogModelIds = new Set<string>();
let catalogFetchedCount = 0;
let catalogStatusHasLegacy = false;
let expandedCatalogModelIds = new Set<string>();

export interface ProviderCatalogState {
  catalogModels: ProviderCatalogModel[];
  selectedCatalogModelIds: ReadonlySet<string>;
  catalogReasoningLevelsByModel: ReadonlyMap<string, ReadonlySet<ConfigurableReasoningLevel>>;
  catalogCustomReasoningByModel: ReadonlyMap<string, string>;
  catalogVisionEnabledModelIds: ReadonlySet<string>;
  catalogToolsEnabledModelIds: ReadonlySet<string>;
  catalogReasoningEnabledModelIds: ReadonlySet<string>;
  catalogTokenLimitsByModel: ReadonlyMap<string, ModelTokenLimits>;
  catalogCheckpointOverridesByModel: ReadonlyMap<string, ModelCheckpointOverride | null>;
  changedCatalogTokenLimitModelIds: ReadonlySet<string>;
  changedCatalogCheckpointOverrideModelIds: ReadonlySet<string>;
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
    catalogCheckpointOverridesByModel,
    changedCatalogTokenLimitModelIds,
    changedCatalogCheckpointOverrideModelIds,
    changedCatalogCapabilityModelIds,
    changedCatalogReasoningModelIds,
    legacyCatalogModelIds,
  };
}

function catalogCapability(
  model: ProviderCatalogModel,
  name: "vision" | "tools",
): boolean | undefined {
  const capabilities = model.capabilities;
  if (!capabilities || Array.isArray(capabilities)) return undefined;
  const value = capabilities[name];
  return typeof value === "boolean" ? value : undefined;
}

function resolvedTokenLimitSource(
  catalogValue: number | undefined,
  existingValue: number | null | undefined,
  existingSource: TokenLimitSource | undefined,
): TokenLimitSource {
  if (catalogValue !== undefined) return "catalog";
  if (existingValue !== null && existingValue !== undefined) {
    return existingSource ?? "unknown";
  }
  return "estimated";
}

function resolvedCatalogTokenLimits(
  model: ProviderCatalogModel,
  existing?: ModelTokenLimits,
): ModelTokenLimits {
  const resolved = resolveCatalogTokenLimits(model, existing);
  const contextWindow = catalogContextWindow(model);
  return {
    ...resolved,
    context_window_source: resolvedTokenLimitSource(
      contextWindow,
      existing?.context_window,
      existing?.context_window_source,
    ),
    input_token_limit_source: resolvedTokenLimitSource(
      model.inputTokenLimit,
      existing?.input_token_limit,
      existing?.input_token_limit_source,
    ),
    output_token_limit_source: resolvedTokenLimitSource(
      model.outputTokenLimit,
      existing?.output_token_limit,
      existing?.output_token_limit_source,
    ),
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

function cloneCheckpointOverride(
  override: ModelCheckpointOverride | null | undefined,
): ModelCheckpointOverride | null {
  return override ? { ...override } : null;
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
  catalogCheckpointOverridesByModel = new Map();
  changedCatalogTokenLimitModelIds = new Set();
  changedCatalogCheckpointOverrideModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
  expandedCatalogModelIds = new Set();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();

  const stepConfig = element<HTMLElement>("#provider-step-config");
  if (stepConfig) {
    stepConfig.hidden = false;
    stepConfig.classList.add("active");
  }
  const catalogResults = element<HTMLElement>("#catalog-results");
  if (catalogResults) {
    catalogResults.hidden = true;
    catalogResults.classList.remove("active");
  }
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
  expandedCatalogModelIds = new Set();
  changedCatalogTokenLimitModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  const existingUpstreamsByModelId = new Map(
    existingUpstreams.map((upstream) => [upstream.upstream_model_id, upstream]),
  );
  catalogTokenLimitsByModel = new Map(
    catalogModels.map((model) => [
      model.id,
      resolvedCatalogTokenLimits(model, existingUpstreamsByModelId.get(model.id)?.token_limits),
    ]),
  );
  catalogCheckpointOverridesByModel = new Map(
    catalogModels.map((model) => [
      model.id,
      cloneCheckpointOverride(
        existingUpstreamsByModelId.get(model.id)?.checkpoint_override,
      ),
    ]),
  );
  changedCatalogCheckpointOverrideModelIds = new Set();
  catalogVisionEnabledModelIds = new Set(
    catalogModels
      .filter((model) => (
        existingUpstreamsByModelId.get(model.id)?.capabilities.vision
        ?? catalogCapability(model, "vision")
        ?? true
      ))
      .map((model) => model.id),
  );
  catalogToolsEnabledModelIds = new Set(
    catalogModels
      .filter((model) => (
        existingUpstreamsByModelId.get(model.id)?.capabilities.tools
        ?? catalogCapability(model, "tools")
        ?? true
      ))
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

  element<HTMLElement>("#provider-step-config").classList.remove("active");
  element<HTMLElement>("#provider-step-config").hidden = true;
  element<HTMLElement>("#catalog-results").hidden = false;
  element<HTMLElement>("#catalog-results").classList.add("active");

  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>(".provider-modal-body").scrollTop = 0;
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
    estimated_default: t("models.tokenPresetEstimatedDefault"),
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

function checkpointOverrideForModel(modelId: string): ModelCheckpointOverride | null {
  return catalogCheckpointOverridesByModel.get(modelId) ?? null;
}

function checkpointSourceLabel(override: ModelCheckpointOverride | null): string {
  if (override?.kind === "percentage") return t("models.checkpointSourcePercentage");
  if (override?.kind === "custom") return t("models.checkpointSourceCustom");
  return t("models.checkpointSourceGlobal");
}

interface CatalogCheckpointControls {
  element: HTMLDivElement;
  refreshPreview: () => void;
}

function createCheckpointControls(
  model: ProviderCatalogModel,
  selected: boolean,
  context: ProviderCatalogContext,
  onPreviewChange: () => void,
): CatalogCheckpointControls {
  const control = document.createElement("div");
  control.className = "catalog-checkpoint-controls";
  const title = document.createElement("span");
  title.className = "catalog-token-title";
  title.textContent = t("models.checkpointTitle");

  const initialOverride = checkpointOverrideForModel(model.id);
  const initialLimits = catalogTokenLimitsByModel.get(model.id)
    ?? resolvedCatalogTokenLimits(model);
  const inheritedCheckpoint = customModelCheckpointLimits(
    store.config.official_model_settings,
    initialLimits,
    null,
  );
  const initialPercentage = initialOverride?.kind === "percentage"
    ? initialOverride.threshold_percent
    : inheritedCheckpoint
      ? Math.max(
          1,
          Math.min(
            100,
            Math.round(inheritedCheckpoint.threshold / inheritedCheckpoint.max_token_limit * 100),
          ),
        )
      : 80;
  const initialCustom = initialOverride?.kind === "custom"
    ? initialOverride
    : {
        kind: "custom" as const,
        token_threshold: inheritedCheckpoint?.threshold ?? 1,
        max_token_limit: inheritedCheckpoint?.max_token_limit ?? 2,
        max_output_tokens: inheritedCheckpoint?.max_output_tokens ?? 1,
      };

  const modeField = document.createElement("label");
  modeField.className = "catalog-token-field catalog-checkpoint-mode-field";
  const modeLabel = document.createElement("span");
  modeLabel.textContent = t("models.checkpointMode");
  const modeSelect = document.createElement("select");
  modeSelect.className = "catalog-token-input catalog-checkpoint-mode";
  for (const [value, label] of [
    ["global", t("models.checkpointFollowGlobal")],
    ["percentage", t("models.checkpointPercentage")],
    ["custom", t("models.checkpointCustom")],
  ] as const) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    modeSelect.append(option);
  }
  modeSelect.value = initialOverride?.kind ?? "global";
  modeField.append(modeLabel, modeSelect);

  const fields = document.createElement("div");
  fields.className = "catalog-token-fields catalog-checkpoint-fields";
  const percentageField = document.createElement("label");
  percentageField.className = "catalog-token-field";
  const percentageLabel = document.createElement("span");
  percentageLabel.textContent = t("models.checkpointThresholdPercentage");
  const percentageInput = document.createElement("input");
  percentageInput.className = "catalog-token-input catalog-checkpoint-number";
  percentageInput.type = "number";
  percentageInput.min = "1";
  percentageInput.max = "100";
  percentageInput.step = "1";
  percentageInput.inputMode = "numeric";
  percentageInput.value = String(initialPercentage);
  percentageField.append(percentageLabel, percentageInput);

  const customFields = document.createElement("div");
  customFields.className = "catalog-token-fields catalog-checkpoint-custom-fields";
  const customInputs: Array<{
    field: "token_threshold" | "max_token_limit" | "max_output_tokens";
    input: HTMLInputElement;
  }> = [];
  for (const [field, label, value] of [
    ["token_threshold", t("models.checkpointThreshold"), initialCustom.token_threshold],
    ["max_token_limit", t("models.checkpointHardLimit"), initialCustom.max_token_limit],
    ["max_output_tokens", t("models.checkpointOutputReserve"), initialCustom.max_output_tokens],
  ] as const) {
    const fieldRow = document.createElement("label");
    fieldRow.className = "catalog-token-field";
    const fieldLabel = document.createElement("span");
    fieldLabel.textContent = label;
    const input = document.createElement("input");
    input.className = "catalog-token-input catalog-checkpoint-number";
    input.type = "number";
    input.min = "1";
    input.max = String(0xffffffff);
    input.step = "1";
    input.inputMode = "numeric";
    input.value = String(value);
    fieldRow.append(fieldLabel, input);
    customFields.append(fieldRow);
    customInputs.push({ field, input });
  }
  fields.append(percentageField, customFields);

  const preview = document.createElement("p");
  preview.className = "catalog-checkpoint-preview";
  preview.setAttribute("role", "status");

  const numberValue = (input: HTMLInputElement): number => {
    const value = Number(input.value);
    return Number.isFinite(value) ? value : 0;
  };
  const overrideFromInputs = (): ModelCheckpointOverride | null => {
    if (modeSelect.value === "global") return null;
    if (modeSelect.value === "percentage") {
      return {
        kind: "percentage",
        threshold_percent: numberValue(percentageInput),
      };
    }
    return {
      kind: "custom",
      token_threshold: numberValue(customInputs[0].input),
      max_token_limit: numberValue(customInputs[1].input),
      max_output_tokens: numberValue(customInputs[2].input),
    };
  };

  const updateFieldState = () => {
    const percentageActive = selected && modeSelect.value === "percentage";
    const customActive = selected && modeSelect.value === "custom";
    modeSelect.disabled = !selected;
    percentageField.hidden = modeSelect.value !== "percentage";
    percentageInput.disabled = !percentageActive;
    customFields.hidden = modeSelect.value !== "custom";
    for (const { input } of customInputs) input.disabled = !customActive;

    const override = overrideFromInputs();
    const valid = isValidModelCheckpointOverride(override);
    percentageInput.setCustomValidity(
      percentageActive && !valid ? t("models.checkpointPercentageInvalid") : "",
    );
    for (const { input } of customInputs) {
      input.setCustomValidity(customActive && !valid ? t("models.checkpointCustomInvalid") : "");
    }
  };

  const refreshPreview = () => {
    updateFieldState();
    const override = checkpointOverrideForModel(model.id);
    const valid = isValidModelCheckpointOverride(override);
    const checkpoint = valid
      ? customModelCheckpointLimits(
          store.config.official_model_settings,
          catalogTokenLimitsByModel.get(model.id) ?? resolvedCatalogTokenLimits(model),
          override,
        )
      : null;
    const source = checkpointSourceLabel(override);
    preview.className = `catalog-checkpoint-preview${!valid ? " invalid" : checkpoint?.clipped ? " clipped" : ""}`;
    if (!valid) {
      preview.textContent = t("models.checkpointInvalidPreview", { source });
    } else if (!checkpoint) {
      preview.textContent = t("models.checkpointUnavailablePreview", { source });
    } else {
      preview.textContent = t("models.checkpointEffectivePreview", {
        threshold: formatTokenLimit(checkpoint.threshold),
        hard: formatTokenLimit(checkpoint.max_token_limit),
        output: formatTokenLimit(checkpoint.max_output_tokens),
        percent: checkpoint.threshold_percent,
        source,
        clipped: checkpoint.clipped ? t("models.checkpointClipped") : t("models.checkpointNotClipped"),
      });
    }
    onPreviewChange();
  };

  const commitOverride = () => {
    catalogCheckpointOverridesByModel.set(model.id, overrideFromInputs());
    changedCatalogCheckpointOverrideModelIds.add(model.id);
    context.setProviderEditorDirty(true);
    refreshPreview();
  };
  modeSelect.addEventListener("change", commitOverride);
  percentageInput.addEventListener("input", () => {
    if (modeSelect.value === "percentage") commitOverride();
  });
  for (const { input } of customInputs) {
    input.addEventListener("input", () => {
      if (modeSelect.value === "custom") commitOverride();
    });
  }

  control.append(title, modeField, fields, preview);
  return { element: control, refreshPreview };
}

function createTokenLimitControls(
  model: ProviderCatalogModel,
  selected: boolean,
  checkpointControlsEnabled: boolean,
  context: ProviderCatalogContext,
  onPreviewChange: () => void,
): HTMLDivElement {
  const control = document.createElement("div");
  control.className = "catalog-token-controls";
  let refreshCheckpointPreview = onPreviewChange;

  const currentLimits = catalogTokenLimitsByModel.get(model.id)
    ?? resolvedCatalogTokenLimits(model);
  const catalogContextLimit = catalogContextWindow(model);
  const hasCatalogContext = catalogContextLimit !== undefined;
  const hasCatalogInput = model.inputTokenLimit !== undefined;
  const hasCatalogOutput = model.outputTokenLimit !== undefined;
  const titleRow = document.createElement("div");
  titleRow.className = "catalog-token-heading";
  const title = document.createElement("span");
  title.className = "catalog-token-title";
  title.textContent = t("models.tokenLimitTitle");
  titleRow.append(title);
  const updateTokenLimit = (
    field: "context_window" | "input_token_limit" | "output_token_limit",
    value: string,
  ) => {
    const trimmed = value.trim();
    const parsed = trimmed.length === 0 ? null : Number(trimmed);
    if (parsed !== null && (!Number.isInteger(parsed) || parsed <= 0 || parsed > 0x7fffffff)) {
      return;
    }
    const next: ModelTokenLimits = {
      ...(catalogTokenLimitsByModel.get(model.id) ?? currentLimits),
      [field]: parsed,
    };
    if (field === "context_window") next.context_window_source = "configured";
    if (field === "input_token_limit") next.input_token_limit_source = "configured";
    if (field === "output_token_limit") next.output_token_limit_source = "configured";
    catalogTokenLimitsByModel.set(model.id, next);
    changedCatalogTokenLimitModelIds.add(model.id);
    context.setProviderEditorDirty(true);
    context.refreshProviderEditorControls();
    refreshCheckpointPreview();
  };

  const fields = document.createElement("div");
  fields.className = "catalog-token-fields";
  const contextSummary = document.createElement("span");
  contextSummary.className = "catalog-token-context";
  const contextParts: string[] = [];
  if (catalogContextLimit !== undefined) {
    contextParts.push(t("models.tokenContextSummary", {
      context: formatTokenLimit(catalogContextLimit),
    }));
  }
  if (model.maxContextWindow !== undefined && model.maxContextWindow !== catalogContextLimit) {
    contextParts.push(t("models.tokenNativeContextSummary", {
      context: formatTokenLimit(model.maxContextWindow),
    }));
  }
  if (model.autoCompactTokenLimit !== undefined) {
    contextParts.push(t("models.tokenAutoCompactSummary", {
      context: formatTokenLimit(model.autoCompactTokenLimit),
    }));
  }
  if (contextParts.length > 0) {
    contextSummary.textContent = contextParts.join(" · ");
  }
  const tokenMeta = document.createElement("div");
  tokenMeta.className = "catalog-token-meta";
  if (contextParts.length > 0) tokenMeta.append(contextSummary);

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
      fieldRow.append(value);
    } else {
      const select = document.createElement("select");
      select.className = "catalog-token-input catalog-token-select catalog-token-limit-select";
      const experienceValues: readonly number[] = field === "input_token_limit"
        ? TOKEN_INPUT_LIMIT_OPTIONS
        : TOKEN_OUTPUT_LIMIT_OPTIONS;
      for (const value of experienceValues) {
        const option = document.createElement("option");
        option.value = String(value);
        option.textContent = formatTokenLimit(value);
        select.append(option);
      }
      const displayedValue = catalogTokenLimitsByModel.get(model.id)?.[field] ?? currentLimits[field];
      const selectedValue = displayedValue ?? DEFAULT_TOKEN_LIMIT;
      if (!experienceValues.includes(selectedValue)) {
        const customOption = document.createElement("option");
        customOption.value = String(selectedValue);
        customOption.textContent = `${formatTokenLimit(selectedValue)} · ${t("models.tokenPresetCustom")}`;
        select.append(customOption);
      }
      select.value = String(selectedValue);
      select.disabled = !selected;
      select.title = t("models.tokenLimitExperienceHint");
      select.addEventListener("change", () => updateTokenLimit(field, select.value));
      fieldRow.append(select);
    }
    fields.append(fieldRow);
  };

  appendField("input_token_limit", model.inputTokenLimit, "tokenInputLimit");
  appendField("output_token_limit", model.outputTokenLimit, "tokenOutputLimit");

  if (!hasCatalogContext) {
    const fieldRow = document.createElement("label");
    fieldRow.className = "catalog-token-field catalog-context-field";
    const fieldLabel = document.createElement("span");
    fieldLabel.textContent = t("models.tokenContextWindow");
    const select = document.createElement("select");
    select.className = "catalog-token-input catalog-context-select";
    for (const value of CONTEXT_WINDOW_OPTIONS) {
      const option = document.createElement("option");
      option.value = String(value);
      option.textContent = formatTokenLimit(value);
      select.append(option);
    }
    const displayedValue = catalogTokenLimitsByModel.get(model.id)?.context_window
      ?? currentLimits.context_window;
    const selectedValue = displayedValue ?? DEFAULT_CONTEXT_WINDOW;
    if (!CONTEXT_WINDOW_OPTIONS.includes(selectedValue as (typeof CONTEXT_WINDOW_OPTIONS)[number])) {
      const customOption = document.createElement("option");
      customOption.value = String(selectedValue);
      customOption.textContent = `${formatTokenLimit(selectedValue)} · ${t("models.tokenPresetCustom")}`;
      select.append(customOption);
    }
    select.value = String(selectedValue);
    select.disabled = !selected;
    select.title = t("models.tokenContextExperienceHint");
    select.addEventListener("change", () => updateTokenLimit("context_window", select.value));
    fieldRow.append(fieldLabel, select);
    fields.append(fieldRow);
  }

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
      const current = catalogTokenLimitsByModel.get(model.id) ?? currentLimits;
      const currentContextWindow = current.context_window ?? DEFAULT_CONTEXT_WINDOW;
      catalogTokenLimitsByModel.set(model.id, {
        ...nextLimits,
        context_window: currentContextWindow,
        context_window_source: current.context_window_source ?? "estimated",
        input_token_limit_source: "configured",
        output_token_limit_source: "configured",
      });
      changedCatalogTokenLimitModelIds.add(model.id);
      fields.querySelectorAll<HTMLSelectElement>(".catalog-token-limit-select").forEach((select, index) => {
        const field = index === 0 ? "input_token_limit" : "output_token_limit";
        const value = nextLimits[field];
        if (value !== null) select.value = String(value);
      });
      context.setProviderEditorDirty(true);
      context.refreshProviderEditorControls();
      refreshCheckpointPreview();
    });
    control.append(titleRow, preset, fields, tokenMeta);
  } else {
    control.append(titleRow, fields, tokenMeta);
  }
  const checkpointControls = createCheckpointControls(
    model,
    checkpointControlsEnabled,
    context,
    onPreviewChange,
  );
  refreshCheckpointPreview = checkpointControls.refreshPreview;
  control.append(checkpointControls.element);
  checkpointControls.refreshPreview();
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
    const selectedLevels = catalogReasoningLevelsByModel.get(model.id);
    const availableReasoningLevels = catalogReasoningLevelsForModel(
      model,
      context.selectedProtocol(),
      existingUpstream,
    );
    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id) && (selectedLevels?.size ?? 0) > 0;
    const expanded = expandedCatalogModelIds.has(model.id);
    row.className = `catalog-model-row${selected ? " selected" : " unselected"}${expanded ? " expanded" : ""}${legacyCatalogModelIds.has(model.id) ? " legacy" : ""}`;
    const select = document.createElement("label");
    select.className = "catalog-model-select";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selected;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        selectedCatalogModelIds.add(model.id);
        expandedCatalogModelIds.add(model.id);
      } else {
        selectedCatalogModelIds.delete(model.id);
        expandedCatalogModelIds.delete(model.id);
      }
      context.setProviderEditorDirty(true);
      renderCatalogModels(context);
    });
    const copy = document.createElement("span");
    copy.className = "catalog-model-copy";
    const nameLine = document.createElement("span");
    nameLine.className = "catalog-model-name-line";
    const name = document.createElement("strong");
    name.textContent = model.displayName;
    nameLine.append(name);
    if (legacyCatalogModelIds.has(model.id)) {
      const legacy = document.createElement("span");
      legacy.className = "legacy-badge";
      legacy.textContent = t("models.currentCatalogMissing");
      legacy.title = t("models.currentCatalogMissingHint");
      nameLine.append(legacy);
    }
    const id = document.createElement("code");
    id.textContent = model.id;
    copy.append(nameLine, id);
    const reasoningMetadataLabel = catalogReasoningMetadataLabel(model);
    if (reasoningMetadataLabel) {
      const reasoningHint = document.createElement("span");
      reasoningHint.className = `catalog-reasoning-hint${model.reasoning?.supported === false ? " unsupported" : ""}`;
      reasoningHint.textContent = reasoningMetadataLabel;
      copy.append(reasoningHint);
    }
    const modelSummary = document.createElement("span");
    modelSummary.className = "catalog-model-summary";
    const tokenSummary = document.createElement("span");
    tokenSummary.className = "catalog-model-summary-item token";
    const checkpointSummary = document.createElement("span");
    const updateTokenAndCheckpointSummary = () => {
      const limits = catalogTokenLimitsByModel.get(model.id) ?? resolvedCatalogTokenLimits(model);
      tokenSummary.textContent = t("models.tokenLimitSummary", {
        input: formatTokenLimit(limits.input_token_limit),
        output: formatTokenLimit(limits.output_token_limit),
      });
      const override = checkpointOverrideForModel(model.id);
      const valid = isValidModelCheckpointOverride(override);
      const checkpoint = valid
        ? customModelCheckpointLimits(
            store.config.official_model_settings,
            limits,
            override,
          )
        : null;
      const source = checkpointSourceLabel(override);
      checkpointSummary.className = `catalog-model-summary-item checkpoint${override ? " active" : ""}${!valid ? " invalid" : ""}`;
      const summaryText = checkpoint
        ? `${t("models.checkpointSummary", {
            threshold: formatTokenLimit(checkpoint.threshold),
            hard: formatTokenLimit(checkpoint.max_token_limit),
            percent: checkpoint.threshold_percent,
            source,
          })}${checkpoint.clipped ? ` · ${t("models.checkpointClipped")}` : ""}`
        : t("models.checkpointSummaryUnavailable", { source });
      checkpointSummary.textContent = summaryText;
      checkpointSummary.title = summaryText;
    };
    updateTokenAndCheckpointSummary();
    const visionSummary = document.createElement("span");
    visionSummary.className = `catalog-model-summary-item${catalogVisionEnabledModelIds.has(model.id) ? " active" : " disabled"}`;
    visionSummary.textContent = t("models.visionInput");
    const toolsSummary = document.createElement("span");
    toolsSummary.className = `catalog-model-summary-item${catalogToolsEnabledModelIds.has(model.id) ? " active" : " disabled"}`;
    toolsSummary.textContent = t("models.toolCalling");
    modelSummary.append(tokenSummary, checkpointSummary, visionSummary, toolsSummary);
    if (reasoningEnabled) {
      const reasoningSummary = document.createElement("span");
      reasoningSummary.className = "catalog-model-summary-item active";
      reasoningSummary.textContent = t("models.reasoningSummary", {
        levels: sortReasoningLevels(selectedLevels!).map(reasoningLevelLabel).join(" · "),
      });
      modelSummary.append(reasoningSummary);
    }
    copy.append(modelSummary);
    select.append(checkbox, copy);

    const capabilities = document.createElement("div");
    capabilities.className = "catalog-model-capabilities";
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
    const expand = document.createElement("button");
    expand.type = "button";
    expand.className = "catalog-model-expand";
    expand.textContent = t(expanded ? "models.collapseModelSettings" : "models.expandModelSettings");
    expand.disabled = !selected;
    expand.setAttribute("aria-expanded", String(expanded));
    expand.addEventListener("click", () => {
      if (expanded) expandedCatalogModelIds.delete(model.id);
      else expandedCatalogModelIds.add(model.id);
      renderCatalogModels(context);
    });
    const headerActions = document.createElement("div");
    headerActions.className = "catalog-model-header-actions";
    headerActions.append(testArea, expand);
    const header = document.createElement("div");
    header.className = "catalog-model-header";
    header.append(select, headerActions);

    const capabilityGroup = document.createElement("div");
    capabilityGroup.className = "catalog-capability-group";
    const capabilityTitle = document.createElement("span");
    capabilityTitle.className = "catalog-capability-title";
    capabilityTitle.textContent = t("models.capabilityColumn");
    capabilityGroup.append(capabilityTitle, capabilities);

    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.hidden = !expanded;
    actions.append(createTokenLimitControls(
      model,
      selected,
      selected && expanded,
      context,
      updateTokenAndCheckpointSummary,
    ));
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
