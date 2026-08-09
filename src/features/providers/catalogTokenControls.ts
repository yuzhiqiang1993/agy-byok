import { t } from "../../i18n";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelTokenLimits } from "../../types/config";
import type { CatalogControlState, ProviderCatalogContext } from "./providerCatalogTypes";
import {
  catalogContextWindow,
  CONTEXT_WINDOW_OPTIONS,
  DEFAULT_CONTEXT_WINDOW,
  DEFAULT_TOKEN_LIMIT,
  formatTokenLimit,
  presetIdForTokenLimits,
  resolveCatalogTokenLimits,
  TOKEN_INPUT_LIMIT_OPTIONS,
  TOKEN_LIMIT_PRESETS,
  TOKEN_OUTPUT_LIMIT_OPTIONS,
  tokenLimitsForPreset,
} from "./modelTokenLimits";

type TokenLimitField = "context_window" | "input_token_limit" | "output_token_limit";
type EditableTokenLimitField = Exclude<TokenLimitField, "context_window">;

function tokenPresetName(id: string): string {
  const labels: Record<string, string> = {
    catalog: t("models.tokenPresetCatalog"),
    estimated_default: t("models.tokenPresetEstimatedDefault"),
    chatgpt_default: t("models.tokenPresetChatgptDefault"),
    chatgpt_thinking: t("models.tokenPresetChatgptThinking"),
    gpt5_api: t("models.tokenPresetGpt5Api"),
    gemini_long: t("models.tokenPresetGeminiLong"),
    claude_long: t("models.tokenPresetClaudeLong"),
    custom: t("models.tokenPresetCustom"),
  };
  return labels[id] ?? id;
}

function createTokenLimitUpdater(
  model: ProviderCatalogModel,
  initialLimits: ModelTokenLimits,
  context: ProviderCatalogContext,
  state: CatalogControlState,
  refreshSummary: () => void,
): (field: TokenLimitField, value: string) => void {
  return (field, value) => {
    const trimmed = value.trim();
    const parsed = trimmed.length === 0 ? null : Number(trimmed);
    if (parsed !== null && (!Number.isInteger(parsed) || parsed <= 0 || parsed > 0x7fffffff)) return;

    const next: ModelTokenLimits = {
      ...(state.catalogTokenLimitsByModel.get(model.id) ?? initialLimits),
      [field]: parsed,
    };
    if (field === "context_window") next.context_window_source = "configured";
    if (field === "input_token_limit") next.input_token_limit_source = "configured";
    if (field === "output_token_limit") next.output_token_limit_source = "configured";
    state.catalogTokenLimitsByModel.set(model.id, next);
    state.changedCatalogTokenLimitModelIds.add(model.id);
    context.setProviderEditorDirty(true);
    context.refreshProviderEditorControls();
    refreshSummary();
  };
}

function createTokenMetadata(
  model: ProviderCatalogModel,
  catalogContextLimit: number | undefined,
): HTMLDivElement {
  const parts: string[] = [];
  if (catalogContextLimit !== undefined) {
    parts.push(t("models.tokenContextSummary", { context: formatTokenLimit(catalogContextLimit) }));
  }
  if (model.maxContextWindow !== undefined && model.maxContextWindow !== catalogContextLimit) {
    parts.push(t("models.tokenNativeContextSummary", {
      context: formatTokenLimit(model.maxContextWindow),
    }));
  }
  if (model.autoCompactTokenLimit !== undefined) {
    parts.push(t("models.tokenAutoCompactSummary", {
      context: formatTokenLimit(model.autoCompactTokenLimit),
    }));
  }

  const metadata = document.createElement("div");
  metadata.className = "catalog-token-meta";
  if (parts.length > 0) {
    const summary = document.createElement("span");
    summary.className = "catalog-token-context";
    summary.textContent = parts.join(" · ");
    metadata.append(summary);
  }
  return metadata;
}

function createEditableTokenLimitField(
  model: ProviderCatalogModel,
  field: EditableTokenLimitField,
  reportedValue: number | undefined,
  labelKey: "models.tokenInputLimit" | "models.tokenOutputLimit",
  selected: boolean,
  currentLimits: ModelTokenLimits,
  state: CatalogControlState,
  updateTokenLimit: (field: TokenLimitField, value: string) => void,
): { element: HTMLLabelElement; select: HTMLSelectElement | undefined } {
  const fieldRow = document.createElement("label");
  fieldRow.className = "catalog-token-field";
  const label = document.createElement("span");
  label.textContent = t(labelKey);
  fieldRow.append(label);
  if (reportedValue !== undefined) {
    const value = document.createElement("span");
    value.className = "catalog-token-value readonly";
    value.textContent = formatTokenLimit(reportedValue);
    value.title = t("models.tokenLimitCatalogValue");
    fieldRow.append(value);
    return { element: fieldRow, select: undefined };
  }

  const select = document.createElement("select");
  select.className = "catalog-token-input catalog-token-select catalog-token-limit-select";
  const options: readonly number[] = field === "input_token_limit"
    ? TOKEN_INPUT_LIMIT_OPTIONS
    : TOKEN_OUTPUT_LIMIT_OPTIONS;
  for (const value of options) {
    const option = document.createElement("option");
    option.value = String(value);
    option.textContent = formatTokenLimit(value);
    select.append(option);
  }
  const displayedValue = state.catalogTokenLimitsByModel.get(model.id)?.[field] ?? currentLimits[field];
  const selectedValue = displayedValue ?? DEFAULT_TOKEN_LIMIT;
  if (!options.includes(selectedValue)) {
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
  return { element: fieldRow, select };
}

function createContextWindowField(
  model: ProviderCatalogModel,
  selected: boolean,
  currentLimits: ModelTokenLimits,
  state: CatalogControlState,
  updateTokenLimit: (field: TokenLimitField, value: string) => void,
): HTMLLabelElement {
  const fieldRow = document.createElement("label");
  fieldRow.className = "catalog-token-field catalog-context-field";
  const label = document.createElement("span");
  label.textContent = t("models.tokenContextWindow");
  const select = document.createElement("select");
  select.className = "catalog-token-input catalog-context-select";
  for (const value of CONTEXT_WINDOW_OPTIONS) {
    const option = document.createElement("option");
    option.value = String(value);
    option.textContent = formatTokenLimit(value);
    select.append(option);
  }
  const displayedValue = state.catalogTokenLimitsByModel.get(model.id)?.context_window
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
  fieldRow.append(label, select);
  return fieldRow;
}

function createTokenPreset(
  model: ProviderCatalogModel,
  selected: boolean,
  currentLimits: ModelTokenLimits,
  fieldSelects: Partial<Record<EditableTokenLimitField, HTMLSelectElement>>,
  context: ProviderCatalogContext,
  state: CatalogControlState,
  refreshSummary: () => void,
): HTMLSelectElement {
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
    const current = state.catalogTokenLimitsByModel.get(model.id) ?? currentLimits;
    state.catalogTokenLimitsByModel.set(model.id, {
      ...nextLimits,
      context_window: current.context_window ?? DEFAULT_CONTEXT_WINDOW,
      context_window_source: current.context_window_source,
      input_token_limit_source: "configured",
      output_token_limit_source: "configured",
    });
    state.changedCatalogTokenLimitModelIds.add(model.id);
    for (const field of ["input_token_limit", "output_token_limit"] as const) {
      const value = nextLimits[field];
      if (value !== null && fieldSelects[field]) fieldSelects[field].value = String(value);
    }
    context.setProviderEditorDirty(true);
    context.refreshProviderEditorControls();
    refreshSummary();
  });
  return preset;
}

function createTokenHeading(): HTMLDivElement {
  const heading = document.createElement("div");
  heading.className = "catalog-token-heading";
  const title = document.createElement("span");
  title.className = "catalog-token-title";
  title.textContent = t("models.tokenLimitTitle");
  heading.append(title);
  return heading;
}

export function createTokenLimitControls(
  model: ProviderCatalogModel,
  selected: boolean,
  context: ProviderCatalogContext,
  state: CatalogControlState,
  onTokenLimitChange: () => void,
): HTMLDivElement {
  const control = document.createElement("div");
  control.className = "catalog-token-controls";
  const refreshSummary = () => onTokenLimitChange();
  const currentLimits = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
  const updateTokenLimit = createTokenLimitUpdater(model, currentLimits, context, state, refreshSummary);
  const fields = document.createElement("div");
  fields.className = "catalog-token-fields";
  const fieldSelects: Partial<Record<EditableTokenLimitField, HTMLSelectElement>> = {};
  const inputField = createEditableTokenLimitField(
    model,
    "input_token_limit",
    model.inputTokenLimit,
    "models.tokenInputLimit",
    selected,
    currentLimits,
    state,
    updateTokenLimit,
  );
  const outputField = createEditableTokenLimitField(
    model,
    "output_token_limit",
    model.outputTokenLimit,
    "models.tokenOutputLimit",
    selected,
    currentLimits,
    state,
    updateTokenLimit,
  );
  fields.append(inputField.element, outputField.element);
  if (inputField.select) fieldSelects.input_token_limit = inputField.select;
  if (outputField.select) fieldSelects.output_token_limit = outputField.select;
  const catalogContextLimit = catalogContextWindow(model);
  if (catalogContextLimit === undefined) {
    fields.append(createContextWindowField(model, selected, currentLimits, state, updateTokenLimit));
  }

  control.append(createTokenHeading());
  if (model.inputTokenLimit === undefined && model.outputTokenLimit === undefined) {
    control.append(createTokenPreset(
      model,
      selected,
      currentLimits,
      fieldSelects,
      context,
      state,
      refreshSummary,
    ));
  }
  control.append(fields, createTokenMetadata(model, catalogContextLimit));
  return control;
}
