import type { Provider, ProviderProtocol } from "../../types/config";
import { store } from "../../store/appStore";
import { setProviderEditorDirtyState } from "./providerState";
import { element } from "../../utils/domUtils";
import { emptyParameters } from "../../utils/modelUtils";
import { t } from "../../i18n";

export let editingProviderId: string | null = null;
let draftProviderId = `provider-${crypto.randomUUID()}`;

interface ProviderFormContext {
  resetCatalogResults: () => void;
  setProviderEditorDirty: (dirty: boolean) => void;
  invalidatePendingProviderSave: () => void;
  refreshProviderEditorControls: () => void;
}

export function selectedProtocol(): ProviderProtocol {
  return element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
}

function protocolDescription(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: t("models.protocolHelpOpenAI"),
    openai_responses: t("models.protocolHelpResponses"),
    anthropic_messages: t("models.protocolHelpAnthropic"),
    gemini_generate_content: t("models.protocolHelpGemini"),
  }[protocol];
}

export function updateProtocolHelp(): void {
  element<HTMLElement>("#protocol-help").textContent = protocolDescription(selectedProtocol());
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

export function updateSuggestedEndpoints(resetCatalogResults: () => void): void {
  const endpoints = suggestedEndpoints(
    element<HTMLInputElement>("#provider-base-url").value,
    selectedProtocol(),
  );
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
    ? store.config.providers.find((item) => item.id === editingProviderId)
    : undefined;

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

export function syncApiKeyToggle(): void {
  const visible = element<HTMLInputElement>("#api-key").type === "text";
  const label = t(visible ? "models.hideKey" : "models.showKey");
  const button = element<HTMLButtonElement>("#toggle-api-key");
  button.textContent = label;
  button.setAttribute("aria-label", label);
  button.setAttribute("aria-pressed", String(visible));
}

export function resetProviderForm(context: ProviderFormContext): void {
  editingProviderId = null;
  draftProviderId = `provider-${crypto.randomUUID()}`;
  element<HTMLFormElement>("#provider-form").reset();
  document.querySelectorAll(".preset-btn").forEach((button) => button.removeAttribute("data-active"));
  element<HTMLInputElement>("#api-key").type = "password";
  syncApiKeyToggle();
  element<HTMLElement>("#provider-form-title").textContent = t("models.addProviderTitle");
  element<HTMLElement>("#provider-form-kicker").textContent = t("models.addKicker");
  element<HTMLSelectElement>("#protocol").value = "openai_chat_completions";
  updateProtocolHelp();
  context.resetCatalogResults();
  setProviderEditorDirtyState(false);
  element<HTMLElement>("#provider-editor-dirty").hidden = true;
  context.invalidatePendingProviderSave();
  context.refreshProviderEditorControls();
}

const PROVIDER_PRESETS = {
  custom: { nameKey: "presets.custom", protocol: "openai_chat_completions", baseUrl: "", category: "recommended local_custom", tagKey: "models.tagCustom", tagClass: "custom-tag", icon: "+", color: "#9333ea", bgColor: "rgba(147, 51, 234, 0.12)" },
  cpa: { nameKey: "presets.cpa", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8317/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "C", color: "#4f46e5", bgColor: "rgba(79, 70, 229, 0.12)" },
  sub2api: { nameKey: "presets.sub2api", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8080/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "S", color: "#6366f1", bgColor: "rgba(99, 102, 241, 0.12)" },
  openrouter: { nameKey: "presets.openrouter", protocol: "openai_chat_completions", baseUrl: "https://openrouter.ai/api/v1", category: "recommended aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "O", color: "#0891b2", bgColor: "rgba(8, 145, 178, 0.12)" },
  modelgate: { nameKey: "presets.modelgate", protocol: "openai_chat_completions", baseUrl: "https://mg.aid.pub/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "M", color: "#0284c7", bgColor: "rgba(2, 132, 199, 0.12)" },
  claude: { nameKey: "presets.claude", protocol: "anthropic_messages", baseUrl: "https://api.anthropic.com", category: "recommended international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "C", color: "#d97706", bgColor: "rgba(217, 119, 6, 0.12)" },
  openai: { nameKey: "presets.openai", protocol: "openai_chat_completions", baseUrl: "https://api.openai.com/v1", category: "recommended international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "O", color: "#059669", bgColor: "rgba(5, 150, 105, 0.12)" },
  gemini: { nameKey: "presets.gemini", protocol: "gemini_generate_content", baseUrl: "https://generativelanguage.googleapis.com", category: "recommended international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "G", color: "#2563eb", bgColor: "rgba(37, 99, 235, 0.12)" },
  deepseek: { nameKey: "presets.deepseek", protocol: "openai_chat_completions", baseUrl: "https://api.deepseek.com", category: "recommended domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "D", color: "#3b82f6", bgColor: "rgba(59, 130, 246, 0.12)" },
  ollama: { nameKey: "presets.ollama", protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:11434/v1", category: "local_custom", tagKey: "models.tagLocal", tagClass: "local-tag", icon: "O", color: "#10b981", bgColor: "rgba(16, 185, 129, 0.12)" },
  siliconflow: { nameKey: "presets.siliconflow", protocol: "openai_chat_completions", baseUrl: "https://api.siliconflow.cn/v1", category: "recommended domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "硅", color: "#2563eb", bgColor: "rgba(37, 99, 235, 0.12)" },
  dashscope: { nameKey: "presets.dashscope", protocol: "openai_chat_completions", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", category: "recommended domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "百", color: "#ea580c", bgColor: "rgba(234, 88, 12, 0.12)" },
  moonshot: { nameKey: "presets.moonshot", protocol: "openai_chat_completions", baseUrl: "https://api.moonshot.cn/v1", category: "recommended domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "K", color: "#e11d48", bgColor: "rgba(225, 29, 72, 0.12)" },
  zhipu: { nameKey: "presets.zhipu", protocol: "openai_chat_completions", baseUrl: "https://open.bigmodel.cn/api/paas/v4", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "智", color: "#3b82f6", bgColor: "rgba(59, 130, 246, 0.12)" },
  minimax: { nameKey: "presets.minimax", protocol: "openai_chat_completions", baseUrl: "https://api.minimaxi.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "M", color: "#8b5cf6", bgColor: "rgba(139, 92, 246, 0.12)" },
  hunyuan: { nameKey: "presets.hunyuan", protocol: "openai_chat_completions", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "混", color: "#0284c7", bgColor: "rgba(2, 132, 199, 0.12)" },
  volcengine: { nameKey: "presets.volcengine", protocol: "openai_chat_completions", baseUrl: "https://ark.cn-beijing.volces.com/api/v3", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "火", color: "#dc2626", bgColor: "rgba(220, 38, 38, 0.12)" },
  qianfan: { nameKey: "presets.qianfan", protocol: "openai_chat_completions", baseUrl: "https://qianfan.baidubce.com/v2", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "千", color: "#2563eb", bgColor: "rgba(37, 99, 235, 0.12)" },
  baichuan: { nameKey: "presets.baichuan", protocol: "openai_chat_completions", baseUrl: "https://api.baichuan-ai.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "百", color: "#f59e0b", bgColor: "rgba(245, 158, 11, 0.12)" },
  yi: { nameKey: "presets.yi", protocol: "openai_chat_completions", baseUrl: "https://api.lingyiwanwu.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "零", color: "#10b981", bgColor: "rgba(16, 185, 129, 0.12)" },
  xunfei: { nameKey: "presets.xunfei", protocol: "openai_chat_completions", baseUrl: "https://spark-api-open.xf-yun.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "星", color: "#0284c7", bgColor: "rgba(2, 132, 199, 0.12)" },
  stepfun: { nameKey: "presets.stepfun", protocol: "openai_chat_completions", baseUrl: "https://api.stepfun.com/v1", category: "domestic", tagKey: "models.tagDomestic", tagClass: "", icon: "阶", color: "#7c3aed", bgColor: "rgba(124, 58, 237, 0.12)" },
  groq: { nameKey: "presets.groq", protocol: "openai_chat_completions", baseUrl: "https://api.groq.com/openai/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "G", color: "#ea580c", bgColor: "rgba(234, 88, 12, 0.12)" },
  github: { nameKey: "presets.github", protocol: "openai_chat_completions", baseUrl: "https://models.inference.ai.azure.com", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "G", color: "#475569", bgColor: "rgba(71, 85, 105, 0.12)" },
  mistral: { nameKey: "presets.mistral", protocol: "openai_chat_completions", baseUrl: "https://api.mistral.ai/v1", category: "international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "M", color: "#ea580c", bgColor: "rgba(234, 88, 12, 0.12)" },
  xai: { nameKey: "presets.xai", protocol: "openai_chat_completions", baseUrl: "https://api.x.ai/v1", category: "international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "X", color: "#0f172a", bgColor: "rgba(15, 23, 42, 0.12)" },
  perplexity: { nameKey: "presets.perplexity", protocol: "openai_chat_completions", baseUrl: "https://api.perplexity.ai", category: "international", tagKey: "models.tagOfficial", tagClass: "official-tag", icon: "P", color: "#0d9488", bgColor: "rgba(13, 148, 136, 0.12)" },
  together: { nameKey: "presets.together", protocol: "openai_chat_completions", baseUrl: "https://api.together.xyz/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "T", color: "#2563eb", bgColor: "rgba(37, 99, 235, 0.12)" },
  fireworks: { nameKey: "presets.fireworks", protocol: "openai_chat_completions", baseUrl: "https://api.fireworks.ai/inference/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "F", color: "#e11d48", bgColor: "rgba(225, 29, 72, 0.12)" },
  cerebras: { nameKey: "presets.cerebras", protocol: "openai_chat_completions", baseUrl: "https://api.cerebras.ai/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "C", color: "#4f46e5", bgColor: "rgba(79, 70, 229, 0.12)" },
  sambanova: { nameKey: "presets.sambanova", protocol: "openai_chat_completions", baseUrl: "https://api.sambanova.ai/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "S", color: "#7c3aed", bgColor: "rgba(124, 58, 237, 0.12)" },
  deepinfra: { nameKey: "presets.deepinfra", protocol: "openai_chat_completions", baseUrl: "https://api.deepinfra.com/v1/openai", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "D", color: "#0284c7", bgColor: "rgba(2, 132, 199, 0.12)" },
  huggingface: { nameKey: "presets.huggingface", protocol: "openai_chat_completions", baseUrl: "https://router.huggingface.co/v1", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "H", color: "#d97706", bgColor: "rgba(217, 119, 6, 0.12)" },
  novita: { nameKey: "presets.novita", protocol: "openai_chat_completions", baseUrl: "https://api.novita.ai/openai", category: "aggregator", tagKey: "models.tagAggregator", tagClass: "", icon: "N", color: "#9333ea", bgColor: "rgba(147, 51, 234, 0.12)" },
} satisfies Record<string, { nameKey: string; protocol: ProviderProtocol; baseUrl: string; category: string; tagKey: string; tagClass: string; icon: string; color: string; bgColor: string }>;

type ProviderPresetKey = keyof typeof PROVIDER_PRESETS;

function isProviderPresetKey(value: string): value is ProviderPresetKey {
  return value in PROVIDER_PRESETS;
}

interface ProviderFormContext {
  resetCatalogResults: () => void;
  setProviderEditorDirty: (dirty: boolean) => void;
  invalidatePendingProviderSave: () => void;
  refreshProviderEditorControls: () => void;
  onPresetSelected?: () => void;
}

export function syncActivePreset(activeKey: string | null): void {
  const cards = document.querySelectorAll<HTMLButtonElement>("#provider-presets .preset-card");
  for (const card of cards) {
    if (activeKey && card.dataset.preset === activeKey) {
      card.setAttribute("data-active", "true");
    } else {
      card.removeAttribute("data-active");
    }
  }
}

export function detectPresetFromUrl(url: string): ProviderPresetKey | null {
  const cleanUrl = url.trim().toLowerCase();
  if (!cleanUrl) return null;
  for (const [key, preset] of Object.entries(PROVIDER_PRESETS)) {
    if (key === "custom" || !preset.baseUrl) continue;
    try {
      const presetHost = new URL(preset.baseUrl).host;
      if (cleanUrl.includes(presetHost)) {
        return key as ProviderPresetKey;
      }
    } catch {
      if (cleanUrl.includes(preset.baseUrl.toLowerCase())) {
        return key as ProviderPresetKey;
      }
    }
  }
  return null;
}

import type { TranslationKey } from "../../i18n";

export function setupProviderPresets(context: ProviderFormContext): void {
  const presetContainer = document.querySelector<HTMLElement>("#provider-presets");
  if (!presetContainer) return;

  const html = Object.entries(PROVIDER_PRESETS).map(([key, item]) => {
    const isCustom = key === "custom";
    const nameStr = t(item.nameKey as TranslationKey);
    const tagStr = t(item.tagKey as TranslationKey);
    const subText = isCustom ? t("models.customCardDesc") : item.baseUrl;
    const customCardClass = isCustom ? " custom-card" : "";
    const tagClass = item.tagClass ? ` ${item.tagClass}` : "";
    const titleAttr = isCustom ? ' data-i18n-title="models.customCardDesc"' : ` title="${subText}"`;

    return `
      <button type="button" class="preset-card${customCardClass}" data-preset="${key}" data-category="${item.category}"${titleAttr}>
        <div class="preset-card-icon" style="background: ${item.bgColor}; color: ${item.color};">${item.icon}</div>
        <span class="preset-card-name" data-i18n="presets.${key}">${nameStr}</span>
        <span class="preset-card-tag${tagClass}" data-i18n="${item.tagKey}">${tagStr}</span>
      </button>
    `;
  }).join("");

  presetContainer.innerHTML = html;

  const cards = presetContainer.querySelectorAll<HTMLButtonElement>(".preset-card");
  const tabButtons = document.querySelectorAll<HTMLButtonElement>(".preset-tab");
  const searchInput = document.querySelector<HTMLInputElement>("#preset-search");

  let activeCategory = "all";
  let searchQuery = "";

  const filterPresets = () => {
    for (const card of cards) {
      const category = card.dataset.category ?? "";
      const text = card.textContent?.toLowerCase() ?? "";
      const presetKey = (card.dataset.preset ?? "").toLowerCase();
      const matchesCategory = activeCategory === "all" || category.includes(activeCategory);
      const matchesSearch = !searchQuery || text.includes(searchQuery) || presetKey.includes(searchQuery);
      card.hidden = !(matchesCategory && matchesSearch);
    }
  };

  for (const tab of tabButtons) {
    tab.addEventListener("click", () => {
      activeCategory = tab.dataset.category ?? "all";
      for (const item of tabButtons) item.classList.remove("active");
      tab.classList.add("active");
      filterPresets();
    });
  }

  if (searchInput) {
    searchInput.addEventListener("input", () => {
      searchQuery = searchInput.value.trim().toLowerCase();
      filterPresets();
    });
  }

  for (const card of cards) {
    card.addEventListener("click", async () => {
      const presetKey = card.dataset.preset;
      if (!presetKey || !isProviderPresetKey(presetKey)) return;
      const preset = PROVIDER_PRESETS[presetKey];


      if (presetKey === "custom") {
        element<HTMLInputElement>("#provider-name").value = "";
        element<HTMLSelectElement>("#protocol").value = "openai_chat_completions";
        element<HTMLInputElement>("#provider-base-url").value = "";
        updateSuggestedEndpoints(context.resetCatalogResults);
        context.setProviderEditorDirty(false);
        syncActivePreset(null);
        context.onPresetSelected?.();
        element<HTMLInputElement>("#provider-name").focus();
        return;
      }

      element<HTMLInputElement>("#provider-name").value = t(`presets.${presetKey}`);
      element<HTMLSelectElement>("#protocol").value = preset.protocol;
      element<HTMLInputElement>("#provider-base-url").value = preset.baseUrl;
      updateSuggestedEndpoints(context.resetCatalogResults);
      context.setProviderEditorDirty(true);

      syncActivePreset(presetKey);
      context.onPresetSelected?.();

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

export function beginProviderEdit(providerId: string | null): void {
  editingProviderId = providerId;
  const provider = providerId
    ? store.config.providers.find((item) => item.id === providerId)
    : undefined;
  if (!provider) return;

  draftProviderId = provider.id;
  element<HTMLElement>("#provider-form-title").textContent = `${t("models.editProviderTitle")} · ${provider.name}`;
  element<HTMLElement>("#provider-form-kicker").textContent = t("models.editKicker");
  element<HTMLInputElement>("#provider-name").value = provider.name;
  element<HTMLSelectElement>("#protocol").value = provider.protocol;
  element<HTMLInputElement>("#provider-base-url").value = inferProviderBase(provider);
  element<HTMLInputElement>("#api-key").value = provider.api_key;
  element<HTMLInputElement>("#models-endpoint").value = provider.models_endpoint;
  element<HTMLInputElement>("#generate-endpoint").value = provider.generate_endpoint;
  updateProtocolHelp();
}
