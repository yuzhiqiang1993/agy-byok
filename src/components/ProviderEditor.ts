import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import type { AppConfig, Provider, ProviderProtocol, UpstreamModel, VirtualModel, ParameterOverrides } from "../types/config";
import type { ProviderCatalogModel } from "../types/catalog";
import type { ProviderChangeSummary, ProviderSavePlan, ModelConnectionTestResult } from "../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningLevel, ReasoningMapping } from "../types/reasoning";
import { store } from "../store/appStore";
import { element, withBusy } from "../utils/domUtils";
import { showNotice } from "./NoticeBar";
import { setProviderEditorActiveTabId } from "./ProviderList";
import { connectionTestResults, providerTestSessions, persistConfig } from "./ProviderCard";
import { stripConfiguredModelSuffix, nextHostModelId, effectiveHostModelId } from "../utils/modelUtils";
import { reasoningLevels, customReasoningMapping, catalogReasoningMetadataLabel, reasoningLevelsForVirtualModels, customReasoningValueFromUpstream, reasoningLevelLabel, sortReasoningLevels, catalogReasoningLevelsForModel } from "../utils/reasoningUtils";
import { openReasoningModal, closeReasoningModal } from "./ReasoningModal";

export let editingProviderId: string | null = null;
export let draftProviderId = `provider-${crypto.randomUUID()}`;
export let catalogModels: ProviderCatalogModel[] = [];
export let selectedCatalogModelIds = new Set<string>();
export let catalogReasoningLevelsByModel = new Map<string, Set<ConfigurableReasoningLevel>>();
export let catalogCustomReasoningByModel = new Map<string, string>();
export let catalogVisionEnabledModelIds = new Set<string>();
export let catalogToolsEnabledModelIds = new Set<string>();
export let catalogReasoningEnabledModelIds = new Set<string>();
export let changedCatalogCapabilityModelIds = new Set<string>();
export let changedCatalogReasoningModelIds = new Set<string>();
export let legacyCatalogModelIds = new Set<string>();

let providerEditorDirty = false;
let providerEditorBusy = false;
let providerEditorReturnFocus: HTMLElement | null = null;
let pendingProviderSavePlan: ProviderSavePlan | null = null;

export function isProviderEditorDirty(): boolean {
  return providerEditorDirty;
}

export function invalidatePendingProviderSave(): void {
  pendingProviderSavePlan = null;
  const providerChangeSummary = element<HTMLElement>("#provider-change-summary");
  providerChangeSummary.hidden = true;
  providerChangeSummary.className = "provider-change-summary";
}

export function setProviderEditorDirty(dirty: boolean): void {
  providerEditorDirty = dirty;
  element<HTMLElement>("#provider-editor-dirty").hidden = !dirty;
  if (dirty) invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

function refreshProviderEditorControls(): void {
  const hasSelection = selectedCatalogModelIds.size > 0;
  const saveProviderButton = element<HTMLButtonElement>("#save-provider");
  const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
  saveProviderButton.disabled = providerEditorBusy || !providerEditorDirty || !hasSelection;
  cancelProviderButton.disabled = providerEditorBusy;
  if (!providerEditorBusy) {
    saveProviderButton.textContent = pendingProviderSavePlan
      ? `确认保存并移除 ${pendingProviderSavePlan.summary.removedVirtualModels.length} 个模型入口`
      : "保存上游服务";
  }
}

function setProviderEditorBusy(busy: boolean): void {
  providerEditorBusy = busy;
  const providerForm = element<HTMLFormElement>("#provider-form");
  const providerList = element<HTMLDivElement>("#provider-list");
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  providerForm.toggleAttribute("inert", busy);
  providerForm.setAttribute("aria-busy", String(busy));
  providerList.toggleAttribute("inert", busy);
  providerFormPanel.dataset.busy = String(busy);
  refreshProviderEditorControls();
}

export async function withProviderEditorBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = "处理中…",
): Promise<void> {
  if (providerEditorBusy) return;
  setProviderEditorBusy(true);
  try {
    await withBusy(button, action, busyLabel);
  } finally {
    setProviderEditorBusy(false);
  }
}

export async function confirmDiscardProviderChanges(): Promise<boolean> {
  if (providerEditorBusy) {
    showNotice("上游服务配置正在处理中，请稍候", "error");
    return false;
  }
  if (!providerEditorDirty) return true;
  try {
    return await confirm("当前有未保存的上游服务修改，确定放弃吗？", { kind: 'warning' });
  } catch (error) {
    console.error("Native confirm dialog failed:", error);
    return window.confirm("当前有未保存的上游服务修改，确定放弃吗？");
  }
}

export function selectedProtocol(): ProviderProtocol {
  return element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
}

function protocolDescription(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: "适用于 /v1/chat/completions 接口，支持 CPA、Sub2API 及主流 OpenAI 兼容服务网关。",
    openai_responses: "适用于 OpenAI Responses API 兼容接口（/v1/responses），请勿误选为 Chat Completions。",
    anthropic_messages: "适用于 /v1/messages 接口，支持 Anthropic 官方 API 及兼容 Messages API 的中转服务。",
    gemini_generate_content: "适用于 Google Gemini 原生 API（:generateContent），支持 /v1beta/models 接口。",
  }[protocol];
}

function updateProtocolHelp(): void {
  const protocol = selectedProtocol();
  element<HTMLElement>("#protocol-help").textContent = protocolDescription(protocol);
}

function suggestedEndpoints(
  baseUrl: string,
  protocol: ProviderProtocol,
): { modelsEndpoint: string; generateEndpoint: string } {
  const base = baseUrl.trim().replace(/\/+$/, "");
  if (!base) return { modelsEndpoint: "", generateEndpoint: "" };
  if (protocol === "gemini_generate_content") {
    const apiBase = base.endsWith("/v1beta") ? base : `${base}/v1beta`;
    return {
      modelsEndpoint: `${apiBase}/models`,
      generateEndpoint: `${apiBase}/models/{model}:generateContent`,
    };
  }
  const apiBase = base.endsWith("/v1") ? base : `${base}/v1`;
  return {
    modelsEndpoint: `${apiBase}/models`,
    generateEndpoint: protocol === "anthropic_messages"
      ? `${apiBase}/messages`
      : protocol === "openai_responses"
        ? `${apiBase}/responses`
        : `${apiBase}/chat/completions`,
  };
}

function updateSuggestedEndpoints(): void {
  const protocol = selectedProtocol();
  const baseUrl = element<HTMLInputElement>("#provider-base-url").value;
  const endpoints = suggestedEndpoints(baseUrl, protocol);
  element<HTMLInputElement>("#models-endpoint").value = endpoints.modelsEndpoint;
  element<HTMLInputElement>("#generate-endpoint").value = endpoints.generateEndpoint;
  updateProtocolHelp();
  resetCatalogResults();
}

export function providerFromForm(): Provider {
  const protocol = selectedProtocol();
  const name = element<HTMLInputElement>("#provider-name").value.trim();
  const generateEndpoint = element<HTMLInputElement>("#generate-endpoint").value.trim();
  const modelsEndpoint = element<HTMLInputElement>("#models-endpoint").value.trim();
  const apiKey = element<HTMLInputElement>("#api-key").value;
  const existing = editingProviderId
    ? store.config?.providers.find((item) => item.id === editingProviderId)
    : undefined;
  
  const emptyParameters = (): ParameterOverrides => ({
    temperature: null,
    max_tokens: null,
    top_p: null,
    top_k: null,
    extra_body: null,
  });

  return {
    id: existing?.id ?? draftProviderId,
    name,
    protocol,
    models_endpoint: modelsEndpoint,
    generate_endpoint: generateEndpoint,
    api_key: apiKey,
    headers: existing?.headers ?? {},
    default_parameters: existing?.default_parameters ?? emptyParameters(),
    connect_timeout_ms: existing?.connect_timeout_ms ?? 5000,
    request_timeout_ms: existing?.request_timeout_ms ?? 120000,
    stream_idle_timeout_ms: existing?.stream_idle_timeout_ms ?? 30000,
    enabled: existing?.enabled ?? true,
  };
}

function resetCatalogResults(): void {
  catalogModels = [];
  selectedCatalogModelIds = new Set();
  catalogReasoningLevelsByModel = new Map();
  catalogCustomReasoningByModel = new Map();
  catalogVisionEnabledModelIds = new Set();
  catalogToolsEnabledModelIds = new Set();
  catalogReasoningEnabledModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
  element<HTMLDivElement>("#catalog-model-list").replaceChildren();
  element<HTMLElement>("#catalog-results").hidden = true;
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  element<HTMLButtonElement>("#save-provider").disabled = true;
}

function resetProviderEditor(): void {
  editingProviderId = null;
  draftProviderId = `provider-${crypto.randomUUID()}`;
  element<HTMLFormElement>("#provider-form").reset();
  document.querySelectorAll(".preset-btn").forEach((button) => button.removeAttribute("data-active"));
  const apiKeyToggle = element<HTMLButtonElement>("#toggle-api-key");
  apiKeyToggle.textContent = "显示";
  apiKeyToggle.setAttribute("aria-pressed", "false");
  apiKeyToggle.setAttribute("aria-label", "显示 API Key");
  element<HTMLInputElement>("#api-key").type = "password";
  element<HTMLElement>("#provider-form-title").textContent = "添加上游服务";
  element<HTMLElement>("#provider-form-kicker").textContent = "ADD UPSTREAM";
  element<HTMLSelectElement>("#protocol").value = "openai_chat_completions";
  updateProtocolHelp();
  resetCatalogResults();
  providerEditorDirty = false;
  element<HTMLElement>("#provider-editor-dirty").hidden = true;
  invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

export async function closeProviderEditor(force = false): Promise<boolean> {
  if (!force && !(await confirmDiscardProviderChanges())) return false;
  const returnFocus = providerEditorReturnFocus;
  providerEditorReturnFocus = null;
  element<HTMLElement>("#provider-form-panel").hidden = true;
  document.body.classList.remove("modal-open");
  resetProviderEditor();
  if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
  return true;
}

const PROVIDER_PRESETS: Record<string, { name: string; protocol: ProviderProtocol; baseUrl: string }> = {
  claude: { name: "Claude 官方", protocol: "anthropic_messages", baseUrl: "https://api.anthropic.com" },
  openai: { name: "OpenAI 官方", protocol: "openai_chat_completions", baseUrl: "https://api.openai.com/v1" },
  gemini: { name: "Gemini 官方", protocol: "gemini_generate_content", baseUrl: "https://generativelanguage.googleapis.com" },
  cpa: { name: "CPA", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8317/v1" },
  sub2api: { name: "Sub2API", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8080/v1" },
  deepseek: { name: "DeepSeek", protocol: "openai_chat_completions", baseUrl: "https://api.deepseek.com" },
  ollama: { name: "Ollama Local", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:11434/v1" },
  openrouter: { name: "OpenRouter", protocol: "openai_chat_completions", baseUrl: "https://openrouter.ai/api/v1" },
  modelgate: { name: "ModelGate", protocol: "openai_chat_completions", baseUrl: "https://mg.aid.pub/v1" },
  groq: { name: "Groq", protocol: "openai_chat_completions", baseUrl: "https://api.groq.com/openai/v1" },
  github: { name: "GitHub Models", protocol: "openai_chat_completions", baseUrl: "https://models.inference.ai.azure.com" },
  siliconflow: { name: "SiliconFlow（硅基流动）", protocol: "openai_chat_completions", baseUrl: "https://api.siliconflow.cn/v1" },
  dashscope: { name: "阿里云百炼", protocol: "openai_chat_completions", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  moonshot: { name: "Moonshot（Kimi）", protocol: "openai_chat_completions", baseUrl: "https://api.moonshot.cn/v1" },
  mistral: { name: "Mistral", protocol: "openai_chat_completions", baseUrl: "https://api.mistral.ai/v1" },
  xai: { name: "xAI", protocol: "openai_chat_completions", baseUrl: "https://api.x.ai/v1" },
  perplexity: { name: "Perplexity", protocol: "openai_chat_completions", baseUrl: "https://api.perplexity.ai" },
  together: { name: "Together AI", protocol: "openai_chat_completions", baseUrl: "https://api.together.xyz/v1" },
  fireworks: { name: "Fireworks AI", protocol: "openai_chat_completions", baseUrl: "https://api.fireworks.ai/inference/v1" },
  cerebras: { name: "Cerebras", protocol: "openai_chat_completions", baseUrl: "https://api.cerebras.ai/v1" },
  sambanova: { name: "SambaNova", protocol: "openai_chat_completions", baseUrl: "https://api.sambanova.ai/v1" },
  deepinfra: { name: "DeepInfra", protocol: "openai_chat_completions", baseUrl: "https://api.deepinfra.com/v1/openai" },
  huggingface: { name: "Hugging Face", protocol: "openai_chat_completions", baseUrl: "https://router.huggingface.co/v1" },
  novita: { name: "Novita AI", protocol: "openai_chat_completions", baseUrl: "https://api.novita.ai/openai" },
  zhipu: { name: "智谱 AI", protocol: "openai_chat_completions", baseUrl: "https://open.bigmodel.cn/api/paas/v4" },
  minimax: { name: "MiniMax", protocol: "openai_chat_completions", baseUrl: "https://api.minimaxi.com/v1" },
  hunyuan: { name: "腾讯混元", protocol: "openai_chat_completions", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1" },
  volcengine: { name: "火山方舟", protocol: "openai_chat_completions", baseUrl: "https://ark.cn-beijing.volces.com/api/v3" },
  qianfan: { name: "百度千帆", protocol: "openai_chat_completions", baseUrl: "https://qianfan.baidubce.com/v2" },
  baichuan: { name: "百川智能", protocol: "openai_chat_completions", baseUrl: "https://api.baichuan-ai.com/v1" },
  yi: { name: "零一万物", protocol: "openai_chat_completions", baseUrl: "https://api.lingyiwanwu.com/v1" },
  xunfei: { name: "讯飞星火", protocol: "openai_chat_completions", baseUrl: "https://spark-api-open.xf-yun.com/v1" },
  stepfun: { name: "阶跃星辰", protocol: "openai_chat_completions", baseUrl: "https://api.stepfun.com/v1" },
  custom: { name: "Custom OpenAI", protocol: "openai_chat_completions", baseUrl: "" },
};

function setupProviderPresets(): void {
  const presetContainer = document.querySelector<HTMLElement>("#provider-presets");
  if (!presetContainer) return;

  const buttons = presetContainer.querySelectorAll<HTMLButtonElement>(".preset-btn");
  for (const button of buttons) {
    button.addEventListener("click", () => {
      const presetKey = button.dataset.preset;
      if (!presetKey) return;
      const preset = PROVIDER_PRESETS[presetKey];
      if (!preset) return;

      element<HTMLInputElement>("#provider-name").value = preset.name;
      element<HTMLSelectElement>("#protocol").value = preset.protocol;
      element<HTMLInputElement>("#provider-base-url").value = preset.baseUrl;
      updateSuggestedEndpoints();
      setProviderEditorDirty(true);

      for (const item of buttons) item.removeAttribute("data-active");
      button.setAttribute("data-active", "true");

      if (presetKey !== "ollama") element<HTMLInputElement>("#api-key").focus();
      else element<HTMLInputElement>("#provider-base-url").focus();
    });
  }
}

function inferProviderBase(provider: Provider): string {
  const suffixes = [
    "/v1/chat/completions",
    "/v1/responses",
    "/v1/messages",
    "/v1beta/models/{model}:generateContent",
  ];
  const suffix = suffixes.find((item) => provider.generate_endpoint.endsWith(item));
  if (suffix) return provider.generate_endpoint.slice(0, -suffix.length);
  try {
    return new URL(provider.generate_endpoint).origin;
  } catch {
    return provider.generate_endpoint;
  }
}

export async function openProviderEditor(providerId: string | null = null): Promise<void> {
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  if (!providerFormPanel.hidden && editingProviderId === providerId) {
    element<HTMLInputElement>("#provider-name").focus();
    return;
  }
  if (!(await confirmDiscardProviderChanges())) return;
  providerEditorReturnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  resetProviderEditor();
  editingProviderId = providerId;
  const provider = providerId
    ? store.config?.providers.find((item) => item.id === providerId)
    : undefined;
  if (provider) {
    draftProviderId = provider.id;
    element<HTMLElement>("#provider-form-title").textContent = `编辑上游服务 · ${provider.name}`;
    element<HTMLElement>("#provider-form-kicker").textContent = "EDIT UPSTREAM";
    element<HTMLInputElement>("#provider-name").value = provider.name;
    element<HTMLSelectElement>("#protocol").value = provider.protocol;
    element<HTMLInputElement>("#provider-base-url").value = inferProviderBase(provider);
    element<HTMLInputElement>("#api-key").value = provider.api_key;
    element<HTMLInputElement>("#models-endpoint").value = provider.models_endpoint;
    element<HTMLInputElement>("#generate-endpoint").value = provider.generate_endpoint;
    updateProtocolHelp();
  }
  providerFormPanel.hidden = false;
  document.body.classList.add("modal-open");
  window.setTimeout(() => element<HTMLInputElement>("#provider-name").focus(), 100);
}

async function fetchProviderCatalog(): Promise<void> {
  const providerForm = element<HTMLFormElement>("#provider-form");
  if (!providerForm.reportValidity()) return;
  invalidatePendingProviderSave();
  refreshProviderEditorControls();
  const provider = providerFromForm();
  const fetched = await invoke<ProviderCatalogModel[]>("fetch_provider_catalog", { provider });
  const fetchedIds = new Set(fetched.map((model) => model.id));
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const existingUpstreams = editingProviderId && store.config
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
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  const existingUpstreamsByModelId = new Map(
    existingUpstreams.map((upstream) => [upstream.upstream_model_id, upstream]),
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
        return upstream
          ? Object.keys(upstream.capabilities.reasoning.levels).length > 0
          : false;
      })
      .map((model) => model.id),
  );
  catalogReasoningLevelsByModel = new Map(catalogModels.map((model) => {
    const upstream = existingUpstreamsByModelId.get(model.id);
    if (!upstream) return [model.id, new Set<ConfigurableReasoningLevel>()];
    const virtualModels = (store.config?.virtual_models || []).filter(
      (item) => item.upstream_model_id === upstream.id,
    );
    return [model.id, reasoningLevelsForVirtualModels(provider.protocol, virtualModels)];
  }));
  catalogCustomReasoningByModel = new Map(
    catalogModels.flatMap((model) => {
      const upstream = existingUpstreamsByModelId.get(model.id);
      const value = upstream ? customReasoningValueFromUpstream(upstream) : null;
      return value ? [[model.id, value] as const] : [];
    }),
  );
  element<HTMLElement>("#catalog-results").hidden = false;
  element<HTMLElement>("#catalog-status").textContent = legacyCatalogModelIds.size > 0
    ? `目录获取成功 · ${fetched.length} 个模型 · ${legacyCatalogModelIds.size} 个已配置模型未返回`
    : `目录获取成功 · ${fetched.length} 个模型`;
  renderCatalogModels();
  element<HTMLElement>("#catalog-results").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function updateCatalogSelection(): void {
  const count = selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent =
    count > 0 ? `已选择 ${count} 个模型` : "未选择任何模型";
  refreshProviderEditorControls();
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

export async function testProviderModelConnection(
  provider: Provider,
  upstreamModelId: string,
  reasoningLevel: ReasoningLevel | null,
  customReasoningValue: string | null
): Promise<ModelConnectionTestResult> {
  return await invoke<ModelConnectionTestResult>("test_provider_model_connection", {
    provider,
    upstreamModelId,
    reasoningLevel,
    customReasoningValue,
  });
}

export function renderCatalogModels(): void {
  const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    const selected = selectedCatalogModelIds.has(model.id);
    const existingUpstream = editingProviderId && store.config
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
      setProviderEditorDirty(true);
      renderCatalogModels();
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
      legacy.textContent = "当前目录未返回";
      legacy.title = "保留选择不会删除现有配置；取消后保存将移除对应模型入口";
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
    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id) && (selectedLevels?.size ?? 0) > 0;
    const reasoningBtn = document.createElement("button");
    reasoningBtn.type = "button";
    reasoningBtn.className = `catalog-reasoning-trigger${reasoningEnabled ? " active" : ""}`;
    const reasoningLevelsSummary = reasoningEnabled
      ? sortReasoningLevels(selectedLevels!).map(reasoningLevelLabel).join(" · ")
      : "";
    reasoningBtn.textContent = reasoningEnabled
      ? `推理强度：${reasoningLevelsSummary}`
      : "配置推理强度";
    const reasoningToggleLabel = catalogReasoningMetadataLabel(model);
    reasoningBtn.title = reasoningToggleLabel ?? "点击配置并测试该模型的推理档位";
    reasoningBtn.disabled = !selected || (model.reasoning?.supported === false && !existingUpstream);
    reasoningBtn.addEventListener("click", () => {
      openReasoningModal(model);
    });

    capabilities.append(
      catalogCapabilityToggle(model.id, "图像输入", catalogVisionEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        setProviderEditorDirty(true);
      }),
      catalogCapabilityToggle(model.id, "工具调用", catalogToolsEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        setProviderEditorDirty(true);
      }),
      reasoningBtn,
    );
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = !selected;
    }

    const test = document.createElement("button");
    test.type = "button";
    test.className = "secondary compact-button";
    test.textContent = "测试";
    test.title = "测试当前模型已勾选的全部推理等级";
    const result = document.createElement("span");
    result.className = "catalog-model-test-result";
    result.setAttribute("role", "status");
    test.addEventListener("click", () => {
      void withProviderEditorBusy(test, async () => {
        const provider = providerFromForm();
        const testCases: Array<{
          label: string;
          reasoningLevel: ReasoningLevel | null;
        }> = [];
        if (catalogReasoningEnabledModelIds.has(model.id)) {
          for (const level of sortReasoningLevels(catalogReasoningLevelsByModel.get(model.id) ?? [])) {
            testCases.push({ label: reasoningLevelLabel(level), reasoningLevel: level });
          }
        }
        if (testCases.length === 0) {
          testCases.push({ label: "普通请求", reasoningLevel: null });
        }

        const results: string[] = [];
        let allSucceeded = true;
        for (const [index, testCase] of testCases.entries()) {
          result.className = "catalog-model-test-result pending";
          result.textContent = `测试中 ${index + 1}/${testCases.length} · ${testCase.label}`;
          const response = await testProviderModelConnection(
            provider,
            model.id,
            testCase.reasoningLevel,
            null
          );
          allSucceeded = allSucceeded && response.success;
          results.push(`${testCase.label}：${response.success ? `通过 · ${response.durationMs} ms` : `失败 · ${response.message}`}`);
        }
        result.className = `catalog-model-test-result ${allSucceeded ? "success" : "error"}`;
        result.textContent = allSucceeded
          ? `全部通过 · ${testCases.length} 项`
          : `测试完成 · ${results.filter((item) => item.includes("失败")).length} 项失败`;
        result.title = results.join("\n");
      }, "测试中…");
    });
    const testArea = document.createElement("div");
    testArea.className = "catalog-model-test-area";
    testArea.append(test, result);
    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.append(capabilities);
    actions.append(testArea);
    row.append(select, actions);
    catalogModelList.append(row);
  }

  if (visibleModels.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state compact-empty";
    empty.textContent = "没有匹配的模型";
    catalogModelList.append(empty);
  }
  updateCatalogSelection();
}

function summarizeProviderChanges(
  providerId: string,
  nextConfig: AppConfig,
): ProviderChangeSummary {
  const currentUpstreams = (store.config?.upstream_models || []).filter((item) => item.provider_id === providerId);
  const nextUpstreams = nextConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const currentUpstreamIds = new Set(currentUpstreams.map((item) => item.id));
  const nextUpstreamIds = new Set(nextUpstreams.map((item) => item.id));
  const currentVirtuals = (store.config?.virtual_models || []).filter(
    (item) => currentUpstreamIds.has(item.upstream_model_id),
  );
  const nextVirtuals = nextConfig.virtual_models.filter(
    (item) => nextUpstreamIds.has(item.upstream_model_id),
  );
  const currentVirtualIds = new Set(currentVirtuals.map((item) => item.id));
  const nextVirtualIds = new Set(nextVirtuals.map((item) => item.id));
  const fallbackBlockers = nextConfig.virtual_models.flatMap((model) =>
    model.fallback_virtual_model_id && !nextVirtualIds.has(model.fallback_virtual_model_id)
      && !nextConfig.virtual_models.some((candidate) => candidate.id === model.fallback_virtual_model_id)
      ? [`“${model.display_name}”引用的备用模型将被移除`]
      : []
  );
  return {
    addedUpstreamIds: nextUpstreams
      .filter((item) => !currentUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    removedUpstreamIds: currentUpstreams
      .filter((item) => !nextUpstreamIds.has(item.id))
      .map((item) => item.upstream_model_id),
    addedVirtualModels: nextVirtuals.filter((item) => !currentVirtualIds.has(item.id)),
    removedVirtualModels: currentVirtuals.filter((item) => !nextVirtualIds.has(item.id)),
    retainedVirtualCount: nextVirtuals.filter((item) => currentVirtualIds.has(item.id)).length,
    legacyModelIds: [...legacyCatalogModelIds].filter((id) => selectedCatalogModelIds.has(id)),
    fallbackBlockers,
  };
}

function renderProviderChangeSummary(summary: ProviderChangeSummary): void {
  const providerChangeSummary = element<HTMLElement>("#provider-change-summary");
  providerChangeSummary.replaceChildren();
  providerChangeSummary.hidden = false;
  providerChangeSummary.className = `provider-change-summary${summary.fallbackBlockers.length > 0 ? " blocked" : summary.removedVirtualModels.length > 0 ? " destructive" : ""}`;
  const title = document.createElement("strong");
  title.textContent = summary.fallbackBlockers.length > 0 ? "当前变更无法保存" : "保存影响";
  const list = document.createElement("ul");
  const lines = [
    `上游模型：新增 ${summary.addedUpstreamIds.length}，移除 ${summary.removedUpstreamIds.length}`,
    `模型入口：新增 ${summary.addedVirtualModels.length}，保留 ${summary.retainedVirtualCount}，移除 ${summary.removedVirtualModels.length}`,
  ];
  if (summary.legacyModelIds.length > 0) {
    lines.push(`保留 ${summary.legacyModelIds.length} 个当前目录未返回的已配置模型`);
  }
  for (const blocker of summary.fallbackBlockers) lines.push(blocker);
  for (const line of lines) {
    const item = document.createElement("li");
    item.textContent = line;
    list.append(item);
  }
  if (summary.removedVirtualModels.length > 0) {
    const removed = document.createElement("details");
    const removedSummary = document.createElement("summary");
    removedSummary.textContent = "查看将移除的模型入口";
    const names = document.createElement("p");
    names.textContent = summary.removedVirtualModels.map((model) => model.display_name).join("、");
    removed.append(removedSummary, names);
    providerChangeSummary.append(title, list, removed);
  } else {
    providerChangeSummary.append(title, list);
  }
}

async function executeProviderSave(plan: ProviderSavePlan): Promise<void> {
  setProviderEditorActiveTabId(plan.provider.id);
  
  const currentUpstreamIds = new Set(
    (store.config?.upstream_models || [])
      .filter((upstream) => upstream.provider_id === plan.provider.id)
      .map((upstream) => upstream.id),
  );
  for (const virtualModel of store.config?.virtual_models || []) {
    if (currentUpstreamIds.has(virtualModel.upstream_model_id)) {
      connectionTestResults.delete(virtualModel.id);
    }
  }
  providerTestSessions.delete(plan.provider.id);
  await persistConfig(plan.nextConfig);
  const currentCount = plan.nextConfig.virtual_models.filter((virtualModel) => {
    const upstream = plan.nextConfig.upstream_models.find(
      (item) => item.id === virtualModel.upstream_model_id,
    );
    return upstream?.provider_id === plan.provider.id;
  }).length;
  setProviderEditorDirty(false);
  void closeProviderEditor(true);
  showNotice(`${plan.wasEditing ? "已更新" : "已添加"}上游服务 ${plan.provider.name}：当前 ${currentCount} 个模型入口`);
}

async function saveProvider(): Promise<void> {
  if (pendingProviderSavePlan) {
    const plan = pendingProviderSavePlan;
    pendingProviderSavePlan = null;
    await executeProviderSave(plan);
    return;
  }
  const providerForm = element<HTMLFormElement>("#provider-form");
  if (!providerForm.reportValidity() || selectedCatalogModelIds.size === 0) return;
  const provider = providerFromForm();
  const previousProvider = editingProviderId
    ? store.config?.providers.find((item) => item.id === editingProviderId)
    : undefined;
  const providerUpstreams = (store.config?.upstream_models || []).filter(
    (item) => item.provider_id === provider.id,
  );
  const providerUpstreamIds = new Set(providerUpstreams.map((item) => item.id));
  const remainingUpstreams = (store.config?.upstream_models || []).filter(
    (item) => item.provider_id !== provider.id,
  );
  const remainingVirtuals = (store.config?.virtual_models || []).filter(
    (item) => !providerUpstreamIds.has(item.upstream_model_id),
  );
  const occupiedHostModelIds = new Set(remainingVirtuals.map(effectiveHostModelId));
  const selectedModels = catalogModels.filter((model) =>
    selectedCatalogModelIds.has(model.id)
  );
  if (selectedModels.length === 0) {
    showNotice("当前模型列表中没有有效选项，请重新获取模型", "error");
    return;
  }
  const missingReasoningLevels = selectedModels.find(
    (model) => catalogReasoningEnabledModelIds.has(model.id)
      && (catalogReasoningLevelsByModel.get(model.id)?.size ?? 0) === 0
      && !catalogCustomReasoningByModel.has(model.id),
  );
  if (missingReasoningLevels) {
    showNotice(`“${missingReasoningLevels.displayName}”已开启推理强度，请至少选择一个等级`, "error");
    return;
  }
  const invalidCustomReasoning = selectedModels.find((model) => {
    const value = catalogCustomReasoningByModel.get(model.id);
    return catalogReasoningEnabledModelIds.has(model.id)
      && value !== undefined
      && customReasoningMapping(provider.protocol, value) === null;
  });
  if (invalidCustomReasoning) {
    showNotice(`“${invalidCustomReasoning.displayName}”的自定义推理值不符合当前协议要求`, "error");
    return;
  }
  const protocol = provider.protocol;
  const protocolChanged = previousProvider !== undefined
    && previousProvider.protocol !== provider.protocol;
  const nextUpstreams: UpstreamModel[] = [];
  const nextVirtuals: VirtualModel[] = [];
  const reasoningLevelsForModel = (modelId: string): Set<ConfigurableReasoningLevel> =>
    catalogReasoningEnabledModelIds.has(modelId)
      ? catalogReasoningLevelsByModel.get(modelId) ?? new Set<ConfigurableReasoningLevel>()
      : new Set<ConfigurableReasoningLevel>();

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find(
      (item) => item.upstream_model_id === model.id,
    );
    if (!existingUpstream) continue;

    const existingVirtuals = (store.config?.virtual_models || []).filter(
      (item) => item.upstream_model_id === existingUpstream.id,
    );
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    if (!reasoningChanged) {
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
      continue;
    }

    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id);
    const selectedReasoningLevels = reasoningLevelsForModel(model.id);
    const customReasoningSelected = catalogCustomReasoningByModel.has(model.id);
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      reasoningEnabled
        ? [...selectedReasoningLevels, ...(customReasoningSelected ? ["auto" as const] : [])]
        : [null],
    );
    for (const virtualModel of existingVirtuals) {
      if (retainedReasoningLevels.has(virtualModel.default_reasoning_level)) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
    }
  }

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find(
      (item) => item.upstream_model_id === model.id,
    );
    const existingVirtuals = existingUpstream
      ? (store.config?.virtual_models || []).filter((item) => item.upstream_model_id === existingUpstream.id)
      : [];
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    const capabilitiesChanged = changedCatalogCapabilityModelIds.has(model.id);
    const vision = catalogVisionEnabledModelIds.has(model.id);
    const tools = catalogToolsEnabledModelIds.has(model.id);
    const id = crypto.randomUUID();
    const upstreamId = existingUpstream?.id ?? `upstream-${id}`;

    const emptyParameters = (): ParameterOverrides => ({
      temperature: null,
      max_tokens: null,
      top_p: null,
      top_k: null,
      extra_body: null,
    });

    if (existingUpstream && !reasoningChanged) {
      nextUpstreams.push(capabilitiesChanged
        ? {
            ...existingUpstream,
            capabilities: { ...existingUpstream.capabilities, vision, tools },
          }
        : existingUpstream);
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
        nextVirtuals.push(virtualModel);
      }
      if (existingVirtuals.length > 0) continue;

      nextVirtuals.push({
        id: `custom-${id}`,
        host_model_id: nextHostModelId(occupiedHostModelIds),
        upstream_model_id: upstreamId,
        display_name: model.displayName,
        default_reasoning_level: null,
        parameter_overrides: emptyParameters(),
        fallback_virtual_model_id: null,
        enabled: true,
      });
      continue;
    }

    const reasoningEnabled = catalogReasoningEnabledModelIds.has(model.id);
    const selectedReasoningLevels = reasoningLevelsForModel(model.id);
    const availableMappings = reasoningLevels(protocol);
    const availableReasoningLevels = catalogReasoningLevelsForModel(model, protocol, existingUpstream);
    const customReasoningValue = catalogCustomReasoningByModel.get(model.id);
    const customMapping = customReasoningValue
      ? customReasoningMapping(protocol, customReasoningValue)
      : null;
    const enabledLevels: ReasoningLevel[] = reasoningEnabled
      ? [
          ...sortReasoningLevels(
            [...selectedReasoningLevels].filter((level) => availableReasoningLevels.includes(level)),
          ),
          ...(customMapping ? ["auto" as const] : []),
        ]
      : [];
    const levels: Partial<Record<ReasoningLevel, ReasoningMapping>> = {};
    for (const level of enabledLevels) {
      const mapping = level === "auto"
        ? customMapping
        : (protocolChanged
          ? undefined
          : existingUpstream?.capabilities.reasoning.levels[level])
          ?? availableMappings[level];
      if (mapping) levels[level] = mapping;
    }
    const reasoning = { levels };
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      reasoningEnabled ? [...enabledLevels] : [null],
    );
    for (const virtualModel of existingVirtuals) {
      if (retainedReasoningLevels.has(virtualModel.default_reasoning_level)) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
    }
    nextUpstreams.push(existingUpstream
      ? {
          ...existingUpstream,
          capabilities: { ...existingUpstream.capabilities, vision, tools, reasoning },
        }
      : {
          id: upstreamId,
          provider_id: provider.id,
          upstream_model_id: model.id,
          display_name: model.displayName,
          capabilities: { vision, tools, reasoning },
          parameter_overrides: emptyParameters(),
          enabled: true,
        });

    const desiredReasoningLevels: Array<ReasoningLevel | null> = reasoningEnabled
      ? enabledLevels
      : [null];
    for (const defaultReasoningLevel of desiredReasoningLevels) {
      const matchingVirtuals = existingVirtuals.filter(
        (virtualModel) => virtualModel.default_reasoning_level === defaultReasoningLevel,
      );
      if (matchingVirtuals.length > 0) {
        for (const virtualModel of matchingVirtuals) {
          occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
          nextVirtuals.push(virtualModel);
        }
        continue;
      }

      const virtualId = crypto.randomUUID();
      nextVirtuals.push({
        id: `custom-${virtualId}`,
        host_model_id: nextHostModelId(occupiedHostModelIds),
        upstream_model_id: upstreamId,
        display_name: model.displayName,
        default_reasoning_level: defaultReasoningLevel,
        parameter_overrides: emptyParameters(),
        fallback_virtual_model_id: null,
        enabled: true,
      });
    }
  }

  const providers = editingProviderId
    ? (store.config?.providers || []).map((item) => item.id === provider.id ? provider : item)
    : [...(store.config?.providers || []), provider];
  const providerRenamed = previousProvider !== undefined && previousProvider.name !== provider.name;
  const providerVirtuals = providerRenamed
    ? nextVirtuals.map((virtualModel) => ({
        ...virtualModel,
        display_name: stripConfiguredModelSuffix(
          virtualModel.display_name,
          previousProvider.name,
        ),
      }))
    : nextVirtuals;
  const finalVirtualModels = [...remainingVirtuals, ...providerVirtuals];
  const nextConfig: AppConfig = {
    proxy_port: store.config?.proxy_port ?? 54321,
    providers,
    upstream_models: [...remainingUpstreams, ...nextUpstreams],
    virtual_models: finalVirtualModels,
  };
  const plan: ProviderSavePlan = {
    provider,
    nextConfig,
    summary: summarizeProviderChanges(provider.id, nextConfig),
    wasEditing: editingProviderId !== null,
  };
  renderProviderChangeSummary(plan.summary);
  if (plan.summary.fallbackBlockers.length > 0) {
    showNotice(`无法保存：${plan.summary.fallbackBlockers[0]}`, "error");
    return;
  }
  if (plan.summary.removedVirtualModels.length > 0) {
    pendingProviderSavePlan = plan;
    refreshProviderEditorControls();
    showNotice("请确认保存并移除列出的模型入口", "error");
    return;
  }
  await executeProviderSave(plan);
}

export function setupProviderEditor(): void {
  setupProviderPresets();
  const providerForm = element<HTMLFormElement>("#provider-form");
  const saveProviderButton = element<HTMLButtonElement>("#save-provider");
  
  providerForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void withProviderEditorBusy(saveProviderButton, saveProvider, "保存中…");
  });
  
  element<HTMLButtonElement>("#fetch-provider-models").addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    void withProviderEditorBusy(button, fetchProviderCatalog, "正在获取…");
  });
  
  element<HTMLInputElement>("#provider-name").addEventListener("input", () => {
    setProviderEditorDirty(true);
  });
  element<HTMLInputElement>("#provider-base-url").addEventListener("input", () => {
    updateSuggestedEndpoints();
    setProviderEditorDirty(true);
  });
  element<HTMLSelectElement>("#protocol").addEventListener("change", () => {
    updateSuggestedEndpoints();
    setProviderEditorDirty(true);
  });
  for (const selector of ["#models-endpoint", "#generate-endpoint", "#api-key"]) {
    element<HTMLInputElement>(selector).addEventListener("input", () => {
      resetCatalogResults();
      setProviderEditorDirty(true);
    });
  }
  
  element<HTMLInputElement>("#catalog-search").addEventListener("input", renderCatalogModels);
  element<HTMLInputElement>("#select-all-models").addEventListener("change", (event) => {
    const checkbox = event.currentTarget as HTMLInputElement;
    const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
    const visibleIds = catalogModels
      .filter((model) => `${model.displayName} ${model.id}`.toLowerCase().includes(query))
      .map((model) => model.id);
    for (const id of visibleIds) {
      if (checkbox.checked) selectedCatalogModelIds.add(id);
      else selectedCatalogModelIds.delete(id);
    }
    setProviderEditorDirty(true);
    renderCatalogModels();
  });
  
  element<HTMLButtonElement>("#close-provider-modal").addEventListener("click", () => {
    void closeProviderEditor();
  });
  
  element<HTMLElement>("#provider-modal-backdrop").addEventListener("click", () => {
    void closeProviderEditor();
  });
  
  const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
  const providerFormPanel = element<HTMLElement>("#provider-form-panel");

  document.addEventListener("keydown", (event) => {
    if (!reasoningModal.hidden) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeReasoningModal();
        return;
      }
    }
    if (providerFormPanel.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void closeProviderEditor();
      return;
    }
    if (event.key !== "Tab") return;
  
    const focusable = [...providerFormPanel.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), summary, [tabindex]:not([tabindex="-1"])',
    )].filter((item) => !item.hidden && item.getClientRects().length > 0);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });
  
  window.addEventListener("beforeunload", (event) => {
    if (!providerEditorDirty) return;
    event.preventDefault();
    event.returnValue = "";
  });
  
  element<HTMLButtonElement>("#toggle-api-key").addEventListener("click", (event) => {
    const input = element<HTMLInputElement>("#api-key");
    const button = event.currentTarget as HTMLButtonElement;
    const visible = input.type === "text";
    input.type = visible ? "password" : "text";
    button.textContent = visible ? "显示" : "隐藏";
    button.setAttribute("aria-pressed", String(!visible));
    button.setAttribute("aria-label", visible ? "显示 API Key" : "隐藏 API Key");
  });

  const openProviderFormButton = element<HTMLButtonElement>("#open-provider-form");
  openProviderFormButton.addEventListener("click", () => openProviderEditor());
  
  const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
  cancelProviderButton.addEventListener("click", () => {
    void closeProviderEditor();
  });
}
