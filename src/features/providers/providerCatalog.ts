import { fetchProviderCatalog as fetchProviderCatalogCommand } from "../../controllers/providerController";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  ProviderProtocol,
  UpstreamModel,
  VirtualModel,
} from "../../types/config";
import {
  catalogReasoningIsAuthoritative,
  catalogReasoningLevelsForModel,
  customReasoningValueFromUpstream,
  reasoningLevelsForVirtualModels,
} from "../../utils/reasoningUtils";
import { element } from "../../utils/domUtils";
import { renderCatalogModelList } from "./catalogModelList";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
  ProviderCatalogState,
} from "./providerCatalogTypes";
import {
  MULTIMODAL_INPUT_MODALITIES,
  catalogInputMimeTypes,
  catalogSupportsInput,
  normalizeSelectedInputMimeTypes,
  upstreamInputMimeTypes,
  upstreamSupportsInput,
} from "./modelMediaCapabilities";
import { resolveCatalogTokenLimits } from "./modelTokenLimits";

export type { ProviderCatalogContext, ProviderCatalogState } from "./providerCatalogTypes";

interface InternalProviderCatalogState extends CatalogModelListState {
  catalogCustomReasoningByModel: Map<string, string>;
  fetchedCount: number;
  hasUnavailableModels: boolean;
}

function emptyCatalogState(): InternalProviderCatalogState {
  return {
    catalogModels: [],
    selectedCatalogModelIds: new Set(),
    catalogReasoningLevelsByModel: new Map(),
    catalogCustomReasoningByModel: new Map(),
    catalogThinkingBudgetsByModel: new Map(),
    catalogImageInputModelIds: new Set(),
    catalogAudioInputModelIds: new Set(),
    catalogVideoInputModelIds: new Set(),
    catalogDocumentInputModelIds: new Set(),
    catalogInputMimeTypesByModel: new Map(),
    catalogImageGenerationModelIds: new Set(),
    catalogToolsEnabledModelIds: new Set(),
    catalogReasoningEnabledModelIds: new Set(),
    catalogTokenLimitsByModel: new Map(),
    changedCatalogTokenLimitModelIds: new Set(),
    changedCatalogCapabilityModelIds: new Set(),
    changedCatalogReasoningModelIds: new Set(),
    unavailableCatalogModelIds: new Set(),
    expandedCatalogModelIds: new Set(),
    fetchedCount: 0,
    hasUnavailableModels: false,
  };
}

let catalogState = emptyCatalogState();

export function getProviderCatalogState(): ProviderCatalogState {
  return catalogState;
}

export function setCatalogModelSelection(modelIds: Iterable<string>, selected: boolean): void {
  for (const modelId of modelIds) {
    if (selected) catalogState.selectedCatalogModelIds.add(modelId);
    else catalogState.selectedCatalogModelIds.delete(modelId);
  }
}

function catalogCapability(
  model: ProviderCatalogModel,
  name: "tools",
): boolean | undefined {
  const capabilities = model.capabilities;
  if (!capabilities || Array.isArray(capabilities)) return undefined;
  const value = capabilities[name];
  return typeof value === "boolean" ? value : undefined;
}

function mergedCatalogModels(
  fetched: ProviderCatalogModel[],
  existingUpstreams: UpstreamModel[],
): { models: ProviderCatalogModel[]; unavailableModelIds: Set<string> } {
  const fetchedIds = new Set(fetched.map((model) => model.id));
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const unavailableModelIds = new Set(
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
  return { models: [...byId.values()], unavailableModelIds };
}

function groupVirtualModelsByUpstreamId(): Map<string, VirtualModel[]> {
  const grouped = new Map<string, VirtualModel[]>();
  for (const virtualModel of store.config.virtual_models) {
    const models = grouped.get(virtualModel.upstream_model_id) ?? [];
    models.push(virtualModel);
    grouped.set(virtualModel.upstream_model_id, models);
  }
  return grouped;
}

function loadedCatalogState(
  fetched: ProviderCatalogModel[],
  existingUpstreams: UpstreamModel[],
  protocol: ProviderProtocol,
): InternalProviderCatalogState {
  const { models, unavailableModelIds } = mergedCatalogModels(fetched, existingUpstreams);
  const upstreamByModelId = new Map(
    existingUpstreams.map((upstream) => [upstream.upstream_model_id, upstream]),
  );
  const virtualsByUpstreamId = groupVirtualModelsByUpstreamId();
  const mediaByModel = new Map(models.map((model) => {
    const upstream = upstreamByModelId.get(model.id);
    const sourceMimeTypes = upstream
      ? upstreamInputMimeTypes(upstream)
      : catalogInputMimeTypes(model);
    const supportsImages = upstream
      ? upstreamSupportsInput(upstream, "image")
      : catalogSupportsInput(model, "image");
    const supportsAudio = upstream
      ? upstreamSupportsInput(upstream, "audio")
      : catalogSupportsInput(model, "audio");
    const supportsVideo = upstream
      ? upstreamSupportsInput(upstream, "video")
      : catalogSupportsInput(model, "video");
    const supportsDocuments = upstream
      ? upstreamSupportsInput(upstream, "document")
      : catalogSupportsInput(model, "document");
    const declaredModalities = new Set([
      ...(supportsImages ? ["image" as const] : []),
      ...(supportsAudio ? ["audio" as const] : []),
      ...(supportsVideo ? ["video" as const] : []),
      ...(supportsDocuments ? ["document" as const] : []),
    ]);
    // 尚未保存过的目录模型只要声明任意多模态能力，就默认开放全部输入类型。
    const selectedModalities = !upstream && declaredModalities.size > 0
      ? new Set(MULTIMODAL_INPUT_MODALITIES)
      : declaredModalities;
    return [model.id, {
      supportsImages: selectedModalities.has("image"),
      supportsAudio: selectedModalities.has("audio"),
      supportsVideo: selectedModalities.has("video"),
      supportsDocuments: selectedModalities.has("document"),
      mimeTypes: normalizeSelectedInputMimeTypes(sourceMimeTypes, selectedModalities),
    }] as const;
  }));
  const reasoningLevelsByModel = new Map(models.map((model) => {
    const upstream = upstreamByModelId.get(model.id);
    if (!upstream) {
      const levels = catalogReasoningIsAuthoritative(model)
        ? catalogReasoningLevelsForModel(model, protocol, undefined)
        : [];
      return [model.id, new Set(levels)] as const;
    }
    const virtualModels = virtualsByUpstreamId.get(upstream.id) ?? [];
    return [
      model.id,
      new Set(reasoningLevelsForVirtualModels(protocol, virtualModels)),
    ] as const;
  }));

  return {
    catalogModels: models,
    selectedCatalogModelIds: new Set(existingUpstreams.map((upstream) => upstream.upstream_model_id)),
    catalogReasoningLevelsByModel: reasoningLevelsByModel,
    catalogCustomReasoningByModel: new Map(models.flatMap((model) => {
      const upstream = upstreamByModelId.get(model.id);
      const value = upstream ? customReasoningValueFromUpstream(upstream) : null;
      return value ? [[model.id, value] as const] : [];
    })),
    catalogThinkingBudgetsByModel: new Map(models.map((model) => {
      const upstream = upstreamByModelId.get(model.id);
      const supportsModelBudget = protocol === "gemini_generate_content"
        && (upstream?.capabilities.reasoning.supported ?? model.reasoning?.supported) !== false;
      return [model.id, {
        thinkingBudget: supportsModelBudget
          ? upstream?.capabilities.reasoning.thinking_budget
            ?? model.reasoning?.thinkingBudget
            ?? null
          : null,
        minThinkingBudget: supportsModelBudget
          ? upstream?.capabilities.reasoning.min_thinking_budget
            ?? model.reasoning?.minThinkingBudget
            ?? null
          : null,
      }] as const;
    })),
    catalogImageInputModelIds: new Set(models
      .filter((model) => mediaByModel.get(model.id)?.supportsImages)
      .map((model) => model.id)),
    catalogAudioInputModelIds: new Set(models
      .filter((model) => mediaByModel.get(model.id)?.supportsAudio)
      .map((model) => model.id)),
    catalogVideoInputModelIds: new Set(models
      .filter((model) => mediaByModel.get(model.id)?.supportsVideo)
      .map((model) => model.id)),
    catalogDocumentInputModelIds: new Set(models
      .filter((model) => mediaByModel.get(model.id)?.supportsDocuments)
      .map((model) => model.id)),
    catalogInputMimeTypesByModel: new Map(models.map((model) => [
      model.id,
      new Set(mediaByModel.get(model.id)?.mimeTypes ?? []),
    ])),
    catalogImageGenerationModelIds: new Set(models
      .filter((model) => {
        const upstream = upstreamByModelId.get(model.id);
        return upstream
          ? upstream.capabilities.roles.includes("image_generation")
            || upstream.capabilities.output_modalities.includes("image")
          : model.roles?.includes("image_generation") === true
            || model.outputModalities?.includes("image") === true;
      })
      .map((model) => model.id)),
    catalogToolsEnabledModelIds: new Set(models
      .filter((model) => upstreamByModelId.get(model.id)?.capabilities.tools
        ?? catalogCapability(model, "tools")
        ?? true)
      .map((model) => model.id)),
    catalogReasoningEnabledModelIds: new Set(models
      .filter((model) => {
        const upstream = upstreamByModelId.get(model.id);
        return upstream
          ? upstream.capabilities.reasoning.supported ?? (
              Object.keys(upstream.capabilities.reasoning.levels).length > 0
              || upstream.capabilities.reasoning.thinking_budget != null
              || upstream.capabilities.reasoning.min_thinking_budget != null
            )
          : model.reasoning?.supported ?? (
              catalogReasoningIsAuthoritative(model)
              || (protocol === "gemini_generate_content" && (
                model.reasoning?.thinkingBudget != null
                || model.reasoning?.minThinkingBudget != null
              ))
            );
      })
      .map((model) => model.id)),
    catalogTokenLimitsByModel: new Map(models.map((model) => [
      model.id,
      resolveCatalogTokenLimits(model, upstreamByModelId.get(model.id)?.token_limits),
    ])),
    changedCatalogTokenLimitModelIds: new Set(),
    changedCatalogCapabilityModelIds: new Set(),
    changedCatalogReasoningModelIds: new Set(),
    unavailableCatalogModelIds: unavailableModelIds,
    expandedCatalogModelIds: new Set(),
    fetchedCount: fetched.length,
    hasUnavailableModels: unavailableModelIds.size > 0,
  };
}

export function renderCatalogStatus(): void {
  const status = element<HTMLElement>("#catalog-status");
  status.textContent = catalogState.fetchedCount === 0
    ? t("models.fetching")
    : catalogState.hasUnavailableModels
      ? t("models.catalogFetchedWithUnavailable", {
          count: catalogState.fetchedCount,
          unavailable: catalogState.unavailableCatalogModelIds.size,
        })
      : t("models.catalogFetched", { count: catalogState.fetchedCount });
}

function showProviderConfigStep(): void {
  const configStep = element<HTMLElement>("#provider-step-config");
  configStep.hidden = false;
  configStep.classList.add("active");
  const catalogResults = element<HTMLElement>("#catalog-results");
  catalogResults.hidden = true;
  catalogResults.classList.remove("active");
}

function showCatalogResultsStep(): void {
  const configStep = element<HTMLElement>("#provider-step-config");
  configStep.classList.remove("active");
  configStep.hidden = true;
  const catalogResults = element<HTMLElement>("#catalog-results");
  catalogResults.hidden = false;
  catalogResults.classList.add("active");
}

export function resetCatalogResults(): void {
  catalogState = emptyCatalogState();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();
  showProviderConfigStep();
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  element<HTMLButtonElement>("#save-provider").disabled = true;
  renderCatalogStatus();
}

export async function fetchProviderCatalog(context: ProviderCatalogContext): Promise<void> {
  if (!element<HTMLFormElement>("#provider-form").reportValidity()) return;
  context.invalidatePendingProviderSave();
  context.refreshProviderEditorControls();
  const provider = context.providerFromForm();
  const fetched = await fetchProviderCatalogCommand(provider);
  const editingProviderId = context.getEditingProviderId();
  const existingUpstreams = editingProviderId
    ? store.config.upstream_models.filter((upstream) => upstream.provider_id === editingProviderId)
    : [];
  catalogState = loadedCatalogState(fetched, existingUpstreams, provider.protocol);
  showCatalogResultsStep();
  renderCatalogStatus();
  renderCatalogModels(context);
  element<HTMLElement>(".provider-modal-body").scrollTop = 0;
}

export function updateCatalogSelection(context: ProviderCatalogContext): void {
  const count = catalogState.selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent = count > 0
    ? t("models.selectedModels", { count })
    : t("models.noModelSelected");
  context.refreshProviderEditorControls();
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleIds = catalogState.catalogModels
    .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
    .map((model) => model.id);
  const selectAll = element<HTMLInputElement>("#select-all-models");
  selectAll.checked = visibleIds.length > 0
    && visibleIds.every((id) => catalogState.selectedCatalogModelIds.has(id));
  selectAll.indeterminate = visibleIds.some((id) => catalogState.selectedCatalogModelIds.has(id))
    && !selectAll.checked;
}

export function renderCatalogModels(context: ProviderCatalogContext): void {
  renderCatalogModelList(context, catalogState, () => updateCatalogSelection(context));
}
