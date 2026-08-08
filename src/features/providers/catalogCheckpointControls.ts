import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelCheckpointOverride } from "../../types/config";
import type { CatalogControlState, ProviderCatalogContext } from "./providerCatalogTypes";
import {
  customModelCheckpointLimits,
  formatTokenLimit,
  isValidModelCheckpointOverride,
  resolveCatalogTokenLimits,
} from "./tokenLimits";

type CustomCheckpointField = "token_threshold" | "max_token_limit" | "max_output_tokens";
type CustomCheckpointOverride = Extract<ModelCheckpointOverride, { kind: "custom" }>;

interface CatalogCheckpointControls {
  element: HTMLDivElement;
  refreshPreview: () => void;
}

interface CheckpointInputs {
  modeField: HTMLLabelElement;
  modeSelect: HTMLSelectElement;
  fields: HTMLDivElement;
  percentageField: HTMLLabelElement;
  percentageInput: HTMLInputElement;
  customFields: HTMLDivElement;
  customInputs: Record<CustomCheckpointField, HTMLInputElement>;
}

interface CheckpointInitialValues {
  mode: "global" | ModelCheckpointOverride["kind"];
  percentage: number;
  custom: CustomCheckpointOverride;
}

export function checkpointSourceLabel(override: ModelCheckpointOverride | null): string {
  if (override?.kind === "percentage") return t("models.checkpointSourcePercentage");
  if (override?.kind === "custom") return t("models.checkpointSourceCustom");
  return store.config.official_model_settings.custom_model.profile === "none"
    ? t("models.checkpointSourceGlobalUnset")
    : t("models.checkpointSourceGlobal");
}

function initialCheckpointValues(
  model: ProviderCatalogModel,
  state: CatalogControlState,
): CheckpointInitialValues {
  const override = state.catalogCheckpointOverridesByModel.get(model.id) ?? null;
  const limits = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
  const inherited = customModelCheckpointLimits(store.config.official_model_settings, limits, null);
  const explicitDefaults = inherited ?? customModelCheckpointLimits(
    store.config.official_model_settings,
    limits,
    { kind: "percentage", threshold_percent: 61 },
  );
  const percentage = override?.kind === "percentage"
    ? override.threshold_percent
    : explicitDefaults
      ? Math.max(1, Math.min(100, Math.round(explicitDefaults.threshold / explicitDefaults.max_token_limit * 100)))
      : 80;
  const custom = override?.kind === "custom"
    ? override
    : {
        kind: "custom" as const,
        token_threshold: explicitDefaults?.threshold ?? 1,
        max_token_limit: explicitDefaults?.max_token_limit ?? 2,
        max_output_tokens: explicitDefaults?.max_output_tokens ?? 1,
      };
  return { mode: override?.kind ?? "global", percentage, custom };
}

function createModeField(mode: CheckpointInitialValues["mode"]): {
  field: HTMLLabelElement;
  select: HTMLSelectElement;
} {
  const field = document.createElement("label");
  field.className = "catalog-token-field catalog-checkpoint-mode-field";
  const label = document.createElement("span");
  label.textContent = t("models.checkpointMode");
  const select = document.createElement("select");
  select.className = "catalog-token-input catalog-checkpoint-mode";
  for (const [value, text] of [
    ["global", t("models.checkpointFollowGlobal")],
    ["percentage", t("models.checkpointPercentage")],
    ["custom", t("models.checkpointCustom")],
  ] as const) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = text;
    select.append(option);
  }
  select.value = mode;
  field.append(label, select);
  return { field, select };
}

function createNumberInput(value: number): HTMLInputElement {
  const input = document.createElement("input");
  input.className = "catalog-token-input catalog-checkpoint-number";
  input.type = "number";
  input.min = "1";
  input.max = String(0xffffffff);
  input.step = "1";
  input.inputMode = "numeric";
  input.value = String(value);
  return input;
}

function createCheckpointInputs(initial: CheckpointInitialValues): CheckpointInputs {
  const mode = createModeField(initial.mode);
  const fields = document.createElement("div");
  fields.className = "catalog-token-fields catalog-checkpoint-fields";
  const percentageField = document.createElement("label");
  percentageField.className = "catalog-token-field";
  const percentageLabel = document.createElement("span");
  percentageLabel.textContent = t("models.checkpointThresholdPercentage");
  const percentageInput = createNumberInput(initial.percentage);
  percentageInput.max = "100";
  percentageField.append(percentageLabel, percentageInput);

  const customFields = document.createElement("div");
  customFields.className = "catalog-token-fields catalog-checkpoint-custom-fields";
  const customInputs = {} as Record<CustomCheckpointField, HTMLInputElement>;
  for (const [field, label, value] of [
    ["token_threshold", t("models.checkpointThreshold"), initial.custom.token_threshold],
    ["max_token_limit", t("models.checkpointHardLimit"), initial.custom.max_token_limit],
    ["max_output_tokens", t("models.checkpointOutputReserve"), initial.custom.max_output_tokens],
  ] as const) {
    const fieldRow = document.createElement("label");
    fieldRow.className = "catalog-token-field";
    const fieldLabel = document.createElement("span");
    fieldLabel.textContent = label;
    const input = createNumberInput(value);
    fieldRow.append(fieldLabel, input);
    customFields.append(fieldRow);
    customInputs[field] = input;
  }
  fields.append(percentageField, customFields);
  return {
    modeField: mode.field,
    modeSelect: mode.select,
    fields,
    percentageField,
    percentageInput,
    customFields,
    customInputs,
  };
}

function numericInputValue(input: HTMLInputElement): number {
  const value = Number(input.value);
  return Number.isFinite(value) ? value : 0;
}

function overrideFromInputs(inputs: CheckpointInputs): ModelCheckpointOverride | null {
  if (inputs.modeSelect.value === "global") return null;
  if (inputs.modeSelect.value === "percentage") {
    return {
      kind: "percentage",
      threshold_percent: numericInputValue(inputs.percentageInput),
    };
  }
  return {
    kind: "custom",
    token_threshold: numericInputValue(inputs.customInputs.token_threshold),
    max_token_limit: numericInputValue(inputs.customInputs.max_token_limit),
    max_output_tokens: numericInputValue(inputs.customInputs.max_output_tokens),
  };
}

function updateInputState(inputs: CheckpointInputs, selected: boolean): void {
  const percentageActive = selected && inputs.modeSelect.value === "percentage";
  const customActive = selected && inputs.modeSelect.value === "custom";
  inputs.modeSelect.disabled = !selected;
  inputs.percentageField.hidden = inputs.modeSelect.value !== "percentage";
  inputs.percentageInput.disabled = !percentageActive;
  inputs.customFields.hidden = inputs.modeSelect.value !== "custom";
  for (const input of Object.values(inputs.customInputs)) input.disabled = !customActive;

  const valid = isValidModelCheckpointOverride(overrideFromInputs(inputs));
  inputs.percentageInput.setCustomValidity(
    percentageActive && !valid ? t("models.checkpointPercentageInvalid") : "",
  );
  for (const input of Object.values(inputs.customInputs)) {
    input.setCustomValidity(customActive && !valid ? t("models.checkpointCustomInvalid") : "");
  }
}

function renderCheckpointPreview(
  model: ProviderCatalogModel,
  state: CatalogControlState,
  preview: HTMLParagraphElement,
): void {
  const override = state.catalogCheckpointOverridesByModel.get(model.id) ?? null;
  const valid = isValidModelCheckpointOverride(override);
  const checkpoint = valid
    ? customModelCheckpointLimits(
        store.config.official_model_settings,
        state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model),
        override,
      )
    : null;
  const source = checkpointSourceLabel(override);
  preview.className = `catalog-checkpoint-preview${!valid ? " invalid" : checkpoint?.clipped ? " clipped" : ""}`;
  if (!valid) {
    preview.textContent = t("models.checkpointInvalidPreview", { source });
  } else if (!checkpoint) {
    const isUnset = store.config.official_model_settings.custom_model.profile === "none" && !override;
    preview.textContent = isUnset
      ? t("models.checkpointSummaryUnavailable", { source })
      : t("models.checkpointUnavailablePreview", { source });
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
}

function bindCheckpointInputs(inputs: CheckpointInputs, commit: () => void): void {
  inputs.modeSelect.addEventListener("change", commit);
  inputs.percentageInput.addEventListener("input", () => {
    if (inputs.modeSelect.value === "percentage") commit();
  });
  for (const input of Object.values(inputs.customInputs)) {
    input.addEventListener("input", () => {
      if (inputs.modeSelect.value === "custom") commit();
    });
  }
}

export function createCheckpointControls(
  model: ProviderCatalogModel,
  selected: boolean,
  context: ProviderCatalogContext,
  state: CatalogControlState,
  onPreviewChange: () => void,
): CatalogCheckpointControls {
  const control = document.createElement("div");
  control.className = "catalog-checkpoint-controls";
  const title = document.createElement("span");
  title.className = "catalog-token-title";
  title.textContent = t("models.checkpointTitle");
  const inputs = createCheckpointInputs(initialCheckpointValues(model, state));
  const preview = document.createElement("p");
  preview.className = "catalog-checkpoint-preview";
  preview.setAttribute("role", "status");
  const refreshPreview = () => {
    updateInputState(inputs, selected);
    renderCheckpointPreview(model, state, preview);
    onPreviewChange();
  };
  const commitOverride = () => {
    state.catalogCheckpointOverridesByModel.set(model.id, overrideFromInputs(inputs));
    state.changedCatalogCheckpointOverrideModelIds.add(model.id);
    context.setProviderEditorDirty(true);
    refreshPreview();
  };
  bindCheckpointInputs(inputs, commitOverride);
  control.append(title, inputs.modeField, inputs.fields, preview);
  return { element: control, refreshPreview };
}
