import { t } from "../../i18n";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ConfigurableReasoningLevel } from "../../types/reasoning";
import { createCatalogModelCapabilities } from "./catalogModelCapabilities";
import {
  formatTokenLimit,
  resolveCatalogTokenLimits,
  TOKEN_INPUT_LIMIT_OPTIONS,
  TOKEN_OUTPUT_LIMIT_OPTIONS,
} from "./modelTokenLimits";
import { resolveCatalogModelRowState, type CatalogModelRowState } from "./catalogModelRowState";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";
import { runCatalogModelTests } from "./providerTesting";

function createTestArea(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): HTMLDivElement {
  const { model, existingUpstream } = rowState;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary compact-button";
  button.textContent = t("models.testConnectionShort");
  button.title = t("models.testSelectedReasoning");
  const result = document.createElement("button");
  result.type = "button";
  result.className = "catalog-model-test-result";
  result.setAttribute("aria-live", "polite");
  result.setAttribute("aria-label", t("models.testDebugDetails"));
  result.disabled = true;
  button.addEventListener("click", () => {
    runCatalogModelTests({
      button,
      result,
      modelId: model.id,
      model,
      existingUpstream,
      providerFromForm: context.providerFromForm,
      isReasoningEnabled: () => state.catalogReasoningEnabledModelIds.has(model.id),
      selectedReasoningLevels: () => state.catalogReasoningLevelsByModel.get(model.id)
        ?? new Set<ConfigurableReasoningLevel>(),
      outputTokenLimit: () => state.catalogTokenLimitsByModel.get(model.id)?.output_token_limit
        ?? model.outputTokenLimit
        ?? null,
      runBusy: context.withProviderEditorBusy,
    });
  });
  const area = document.createElement("div");
  area.className = "catalog-model-test-area";
  area.append(button, result);
  return area;
}

function createTokenLimitsControl(
  rowState: CatalogModelRowState,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
): HTMLSpanElement {
  const { model, selected } = rowState;
  const tokenLimits = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
  const container = document.createElement("span");
  container.className = "catalog-token-badge";

  // Case 1: Both reported directly by upstream catalog -> Readonly verified badge
  if (model.inputTokenLimit !== undefined && model.outputTokenLimit !== undefined) {
    container.textContent = t("models.tokenLimitSummary", {
      input: formatTokenLimit(model.inputTokenLimit),
      output: formatTokenLimit(model.outputTokenLimit),
    });
    container.title = t("models.tokenLimitCatalogValue");
    return container;
  }

  // Case 2: Not reported -> Editable inline select controls
  container.classList.add("editable");

  // Input Limit
  const inputLabel = document.createElement("span");
  inputLabel.className = "catalog-token-inline-field";
  const inputPrefix = document.createElement("span");
  inputPrefix.textContent = `${t("models.tokenInputLimit")} `;
  inputLabel.append(inputPrefix);

  if (model.inputTokenLimit !== undefined) {
    const inputVal = document.createElement("span");
    inputVal.textContent = formatTokenLimit(model.inputTokenLimit);
    inputVal.title = t("models.tokenLimitCatalogValue");
    inputLabel.append(inputVal);
  } else {
    const inputSelect = document.createElement("select");
    inputSelect.className = "catalog-token-select";
    inputSelect.disabled = !selected;
    for (const val of TOKEN_INPUT_LIMIT_OPTIONS) {
      const opt = document.createElement("option");
      opt.value = String(val);
      opt.textContent = formatTokenLimit(val);
      inputSelect.append(opt);
    }
    const currentInput = tokenLimits.input_token_limit ?? 128_000;
    if (!TOKEN_INPUT_LIMIT_OPTIONS.includes(currentInput as any)) {
      const customOpt = document.createElement("option");
      customOpt.value = String(currentInput);
      customOpt.textContent = formatTokenLimit(currentInput);
      inputSelect.append(customOpt);
    }
    inputSelect.value = String(currentInput);
    inputSelect.title = t("models.tokenLimitExperienceHint");
    inputSelect.addEventListener("change", () => {
      const num = Number(inputSelect.value);
      if (!Number.isInteger(num) || num <= 0) return;
      const current = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
      state.catalogTokenLimitsByModel.set(model.id, {
        ...current,
        input_token_limit: num,
        input_token_limit_source: "configured",
      });
      state.changedCatalogTokenLimitModelIds.add(model.id);
      context.setProviderEditorDirty(true);
    });
    inputLabel.append(inputSelect);
  }

  const separator = document.createElement("span");
  separator.className = "catalog-token-separator";
  separator.textContent = "·";

  // Output Limit
  const outputLabel = document.createElement("span");
  outputLabel.className = "catalog-token-inline-field";
  const outputPrefix = document.createElement("span");
  outputPrefix.textContent = `${t("models.tokenOutputLimit")} `;
  outputLabel.append(outputPrefix);

  if (model.outputTokenLimit !== undefined) {
    const outputVal = document.createElement("span");
    outputVal.textContent = formatTokenLimit(model.outputTokenLimit);
    outputVal.title = t("models.tokenLimitCatalogValue");
    outputLabel.append(outputVal);
  } else {
    const outputSelect = document.createElement("select");
    outputSelect.className = "catalog-token-select";
    outputSelect.disabled = !selected;
    for (const val of TOKEN_OUTPUT_LIMIT_OPTIONS) {
      const opt = document.createElement("option");
      opt.value = String(val);
      opt.textContent = formatTokenLimit(val);
      outputSelect.append(opt);
    }
    const currentOutput = tokenLimits.output_token_limit ?? 65_536;
    if (!TOKEN_OUTPUT_LIMIT_OPTIONS.includes(currentOutput as any)) {
      const customOpt = document.createElement("option");
      customOpt.value = String(currentOutput);
      customOpt.textContent = formatTokenLimit(currentOutput);
      outputSelect.append(customOpt);
    }
    outputSelect.value = String(currentOutput);
    outputSelect.title = t("models.tokenLimitExperienceHint");
    outputSelect.addEventListener("change", () => {
      const num = Number(outputSelect.value);
      if (!Number.isInteger(num) || num <= 0) return;
      const current = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
      state.catalogTokenLimitsByModel.set(model.id, {
        ...current,
        output_token_limit: num,
        output_token_limit_source: "configured",
      });
      state.changedCatalogTokenLimitModelIds.add(model.id);
      context.setProviderEditorDirty(true);
    });
    outputLabel.append(outputSelect);
  }

  container.append(inputLabel, separator, outputLabel);
  return container;
}

export function createCatalogModelRow(
  model: ProviderCatalogModel,
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  rerender: () => void,
): HTMLDivElement {
  const rowState = resolveCatalogModelRowState(model, context, state);
  const { selected } = rowState;
  const focused = context.getFocusedCatalogModelId() === model.id;
  const row = document.createElement("div");
  row.className = `catalog-model-row${selected ? " selected" : " unselected"}${state.unavailableCatalogModelIds.has(model.id) ? " unavailable" : ""}${focused ? " focused-editor" : ""}`;

  // Top Row: Selection (Checkbox + Model Name + Unavailable Badge) + Test Area
  const topRow = document.createElement("div");
  topRow.className = "catalog-model-top-row";

  const select = focused ? document.createElement("div") : document.createElement("label");
  select.className = "catalog-model-select";
  const checkbox = focused ? null : document.createElement("input");
  if (checkbox) {
    checkbox.type = "checkbox";
    checkbox.checked = selected;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        state.selectedCatalogModelIds.add(model.id);
      } else {
        state.selectedCatalogModelIds.delete(model.id);
      }
      context.setProviderEditorDirty(true);
      rerender();
    });
  }

  const isImage = state.catalogImageGenerationModelIds.has(model.id);

  const nameLine = document.createElement("span");
  nameLine.className = "catalog-model-name-line";
  const name = document.createElement("strong");
  name.textContent = model.displayName;
  nameLine.append(name);

  if (isImage) {
    const iconSvg = `<svg class="catalog-model-type-icon" viewBox="0 0 16 16" width="10" height="10" fill="currentColor"><path d="M8 0a1.5 1.5 0 0 1 1.415 1.002l.504 1.512a2.5 2.5 0 0 0 1.567 1.567l1.512.504a1.5 1.5 0 0 1 0 2.83l-1.512.504a2.5 2.5 0 0 0-1.567 1.567l-.504 1.512a1.5 1.5 0 0 1-2.83 0l-.504-1.512a2.5 2.5 0 0 0-1.567-1.567l-1.512-.504a1.5 1.5 0 0 1 0-2.83l1.512-.504a2.5 2.5 0 0 0 1.567-1.567l.504-1.512A1.5 1.5 0 0 1 8 0z"/></svg>`;

    if (!selected) {
      const typeBadge = document.createElement("span");
      typeBadge.className = "catalog-model-type-badge";
      typeBadge.innerHTML = `${iconSvg}<span>${t("models.imageModelType")}</span>`;
      nameLine.append(typeBadge);
    } else {
      const typeSelectContainer = document.createElement("div");
      typeSelectContainer.className = "catalog-model-type-select-container";
      typeSelectContainer.innerHTML = iconSvg;

      const typeSelect = document.createElement("select");
      typeSelect.className = "catalog-model-type-select";
      const imageOption = document.createElement("option");
      imageOption.value = "image";
      imageOption.textContent = t("models.imageModelType");
      const chatOption = document.createElement("option");
      chatOption.value = "chat";
      chatOption.textContent = t("models.switchToChatModel");
      typeSelect.append(imageOption, chatOption);
      typeSelect.value = "image";
      typeSelect.addEventListener("click", (event) => {
        event.stopPropagation();
      });
      typeSelect.addEventListener("change", (event) => {
        event.stopPropagation();
        const nextKind = typeSelect.value as "chat" | "image";
        if (nextKind === "chat") {
          state.catalogImageGenerationModelIds.delete(model.id);
          state.catalogToolsEnabledModelIds.add(model.id);
          state.changedCatalogCapabilityModelIds.add(model.id);
          context.setProviderEditorDirty(true);
          rerender();
        }
      });
      typeSelectContainer.append(typeSelect);
      nameLine.append(typeSelectContainer);
    }
  }

  if (state.unavailableCatalogModelIds.has(model.id)) {
    const unavailableBadge = document.createElement("span");
    unavailableBadge.className = "unavailable-badge";
    unavailableBadge.textContent = t("models.currentCatalogMissing");
    unavailableBadge.title = t("models.currentCatalogMissingHint");
    nameLine.append(unavailableBadge);
  }
  if (checkbox) select.append(checkbox);
  select.append(nameLine);

  const testArea = createTestArea(rowState, context, state);
  topRow.append(select, testArea);

  if (!isImage) {
    const bottomRow = document.createElement("div");
    bottomRow.className = "catalog-model-bottom-row";
    const tokenLimitsControl = createTokenLimitsControl(rowState, context, state);
    bottomRow.append(tokenLimitsControl);
    const capabilities = createCatalogModelCapabilities(rowState, context, state, rerender);
    bottomRow.append(capabilities);
    row.append(topRow, bottomRow);
  } else {
    row.append(topRow);
  }
  return row;
}
