import type { Provider, ProviderProtocol } from "../../types/config";
import { confirmHostAction } from "../../components/ConfirmModal";
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
  claude: { protocol: "anthropic_messages", baseUrl: "https://api.anthropic.com" },
  openai: { protocol: "openai_chat_completions", baseUrl: "https://api.openai.com/v1" },
  gemini: { protocol: "gemini_generate_content", baseUrl: "https://generativelanguage.googleapis.com" },
  cpa: { protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8317/v1" },
  sub2api: { protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:8080/v1" },
  deepseek: { protocol: "openai_chat_completions", baseUrl: "https://api.deepseek.com" },
  ollama: { protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:11434/v1" },
  openrouter: { protocol: "openai_chat_completions", baseUrl: "https://openrouter.ai/api/v1" },
  modelgate: { protocol: "openai_chat_completions", baseUrl: "https://mg.aid.pub/v1" },
  groq: { protocol: "openai_chat_completions", baseUrl: "https://api.groq.com/openai/v1" },
  github: { protocol: "openai_chat_completions", baseUrl: "https://models.inference.ai.azure.com" },
  siliconflow: { protocol: "openai_chat_completions", baseUrl: "https://api.siliconflow.cn/v1" },
  dashscope: { protocol: "openai_chat_completions", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  moonshot: { protocol: "openai_chat_completions", baseUrl: "https://api.moonshot.cn/v1" },
  mistral: { protocol: "openai_chat_completions", baseUrl: "https://api.mistral.ai/v1" },
  xai: { protocol: "openai_chat_completions", baseUrl: "https://api.x.ai/v1" },
  perplexity: { protocol: "openai_chat_completions", baseUrl: "https://api.perplexity.ai" },
  together: { protocol: "openai_chat_completions", baseUrl: "https://api.together.xyz/v1" },
  fireworks: { protocol: "openai_chat_completions", baseUrl: "https://api.fireworks.ai/inference/v1" },
  cerebras: { protocol: "openai_chat_completions", baseUrl: "https://api.cerebras.ai/v1" },
  sambanova: { protocol: "openai_chat_completions", baseUrl: "https://api.sambanova.ai/v1" },
  deepinfra: { protocol: "openai_chat_completions", baseUrl: "https://api.deepinfra.com/v1/openai" },
  huggingface: { protocol: "openai_chat_completions", baseUrl: "https://router.huggingface.co/v1" },
  novita: { protocol: "openai_chat_completions", baseUrl: "https://api.novita.ai/openai" },
  zhipu: { protocol: "openai_chat_completions", baseUrl: "https://open.bigmodel.cn/api/paas/v4" },
  minimax: { protocol: "openai_chat_completions", baseUrl: "https://api.minimaxi.com/v1" },
  hunyuan: { protocol: "openai_chat_completions", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1" },
  volcengine: { protocol: "openai_chat_completions", baseUrl: "https://ark.cn-beijing.volces.com/api/v3" },
  qianfan: { protocol: "openai_chat_completions", baseUrl: "https://qianfan.baidubce.com/v2" },
  baichuan: { protocol: "openai_chat_completions", baseUrl: "https://api.baichuan-ai.com/v1" },
  yi: { protocol: "openai_chat_completions", baseUrl: "https://api.lingyiwanwu.com/v1" },
  xunfei: { protocol: "openai_chat_completions", baseUrl: "https://spark-api-open.xf-yun.com/v1" },
  stepfun: { protocol: "openai_chat_completions", baseUrl: "https://api.stepfun.com/v1" },
  custom: { protocol: "openai_chat_completions", baseUrl: "" },
} satisfies Record<string, { protocol: ProviderProtocol; baseUrl: string }>;

type ProviderPresetKey = keyof typeof PROVIDER_PRESETS;

function isProviderPresetKey(value: string): value is ProviderPresetKey {
  return value in PROVIDER_PRESETS;
}

export function setupProviderPresets(context: ProviderFormContext): void {
  const presetContainer = document.querySelector<HTMLElement>("#provider-presets");
  if (!presetContainer) return;

  const buttons = presetContainer.querySelectorAll<HTMLButtonElement>(".preset-btn");
  const tabButtons = document.querySelectorAll<HTMLButtonElement>(".preset-tab");
  const searchInput = document.querySelector<HTMLInputElement>("#preset-search");

  let activeCategory = "all";
  let searchQuery = "";

  const filterPresets = () => {
    for (const button of buttons) {
      const category = button.dataset.category ?? "";
      const text = button.textContent?.toLowerCase() ?? "";
      const presetKey = (button.dataset.preset ?? "").toLowerCase();
      const matchesCategory = activeCategory === "all" || category.includes(activeCategory);
      const matchesSearch = !searchQuery || text.includes(searchQuery) || presetKey.includes(searchQuery);
      button.hidden = !(matchesCategory && matchesSearch);
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

  for (const button of buttons) {
    button.addEventListener("click", async () => {
      const presetKey = button.dataset.preset;
      if (!presetKey || !isProviderPresetKey(presetKey)) return;
      const preset = PROVIDER_PRESETS[presetKey];

      const currentBaseUrl = element<HTMLInputElement>("#provider-base-url").value.trim();
      const currentApiKey = element<HTMLInputElement>("#api-key").value.trim();
      if (currentBaseUrl || currentApiKey) {
        const confirmed = await confirmHostAction(
          t("models.presetOverwriteConfirm"),
          t("modal.confirmTitle"),
          t("models.confirmOverwrite"),
          t("models.cancel")
        );
        if (!confirmed) return;
      }

      element<HTMLInputElement>("#provider-name").value = t(`presets.${presetKey}`);
      element<HTMLSelectElement>("#protocol").value = preset.protocol;
      element<HTMLInputElement>("#provider-base-url").value = preset.baseUrl;
      updateSuggestedEndpoints(context.resetCatalogResults);
      context.setProviderEditorDirty(true);

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
