import { t } from "../../i18n";
import type { ProviderCatalogModel } from "../../types/catalog";
import {
  catalogReasoningMetadataLabel,
  reasoningLevelLabel,
  sortReasoningLevels,
} from "../../utils/reasoningUtils";
import type { CatalogModelRowState } from "./catalogModelRowState";
import type { CatalogModelListState } from "./providerCatalogTypes";
import { formatTokenLimit, resolveCatalogTokenLimits } from "./tokenLimits";

export interface CatalogModelSummary {
  element: HTMLSpanElement;
  refreshToken: () => void;
}

function createTokenSummary(
  model: ProviderCatalogModel,
  state: CatalogModelListState,
): Pick<CatalogModelSummary, "refreshToken"> & { token: HTMLSpanElement } {
  const token = document.createElement("span");
  token.className = "catalog-model-summary-item token";
  const refreshToken = () => {
    const limits = state.catalogTokenLimitsByModel.get(model.id) ?? resolveCatalogTokenLimits(model);
    token.textContent = t("models.tokenLimitSummary", {
      input: formatTokenLimit(limits.input_token_limit),
      output: formatTokenLimit(limits.output_token_limit),
    });
  };

  refreshToken();
  return { token, refreshToken };
}

function createModelSummary(
  rowState: CatalogModelRowState,
  state: CatalogModelListState,
): CatalogModelSummary {
  const { model, reasoningEnabled, selectedReasoningLevels } = rowState;
  const summary = document.createElement("span");
  summary.className = "catalog-model-summary";
  const { token, refreshToken } = createTokenSummary(model, state);
  const vision = document.createElement("span");
  vision.className = `catalog-model-summary-item${state.catalogVisionEnabledModelIds.has(model.id) ? " active" : " disabled"}`;
  vision.textContent = t("models.visionInput");
  const tools = document.createElement("span");
  tools.className = `catalog-model-summary-item${state.catalogToolsEnabledModelIds.has(model.id) ? " active" : " disabled"}`;
  tools.textContent = t("models.toolCalling");
  summary.append(token, vision, tools);

  if (reasoningEnabled && selectedReasoningLevels) {
    const reasoning = document.createElement("span");
    reasoning.className = "catalog-model-summary-item active";
    reasoning.textContent = t("models.reasoningSummary", {
      levels: sortReasoningLevels(selectedReasoningLevels).map(reasoningLevelLabel).join(" · "),
    });
    summary.append(reasoning);
  }
  return { element: summary, refreshToken };
}

export function createModelCopy(
  rowState: CatalogModelRowState,
  state: CatalogModelListState,
): { element: HTMLSpanElement; summary: CatalogModelSummary } {
  const { model } = rowState;
  const copy = document.createElement("span");
  copy.className = "catalog-model-copy";
  const nameLine = document.createElement("span");
  nameLine.className = "catalog-model-name-line";
  const name = document.createElement("strong");
  name.textContent = model.displayName;
  nameLine.append(name);
  if (state.unavailableCatalogModelIds.has(model.id)) {
    const unavailableBadge = document.createElement("span");
    unavailableBadge.className = "unavailable-badge";
    unavailableBadge.textContent = t("models.currentCatalogMissing");
    unavailableBadge.title = t("models.currentCatalogMissingHint");
    nameLine.append(unavailableBadge);
  }

  const id = document.createElement("code");
  id.textContent = model.id;
  copy.append(nameLine, id);
  const reasoningMetadata = catalogReasoningMetadataLabel(model);
  if (reasoningMetadata) {
    const reasoningHint = document.createElement("span");
    reasoningHint.className = `catalog-reasoning-hint${model.reasoning?.supported === false ? " unsupported" : ""}`;
    reasoningHint.textContent = reasoningMetadata;
    copy.append(reasoningHint);
  }

  const summary = createModelSummary(rowState, state);
  copy.append(summary.element);
  return { element: copy, summary };
}
