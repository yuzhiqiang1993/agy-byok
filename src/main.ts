import { invoke } from "@tauri-apps/api/core";

type ProviderProtocol = "openai" | "anthropic" | "gemini";
type ReasoningLevel = "low" | "medium" | "high";

type ReasoningMapping =
  | { kind: "effort"; value: string }
  | { kind: "budget_tokens"; value: number }
  | { kind: "native_level"; value: string };

interface ParameterOverrides {
  temperature: number | null;
  max_tokens: number | null;
  top_p: number | null;
  top_k: number | null;
  extra_body: Record<string, unknown> | null;
}

interface Provider {
  id: string;
  name: string;
  protocol: ProviderProtocol;
  models_endpoint: string;
  generate_endpoint: string;
  api_key: string;
  headers: Record<string, string>;
  default_parameters: ParameterOverrides;
  connect_timeout_ms: number;
  request_timeout_ms: number;
  stream_idle_timeout_ms: number;
  enabled: boolean;
}

interface UpstreamModel {
  id: string;
  provider_id: string;
  upstream_model_id: string;
  display_name: string;
  capabilities: {
    vision: boolean;
    tools: boolean;
    reasoning: { levels: Partial<Record<ReasoningLevel, ReasoningMapping>> };
  };
  parameter_overrides: ParameterOverrides;
  enabled: boolean;
}

interface VirtualModel {
  id: string;
  host_model_id: string | null;
  upstream_model_id: string;
  display_name: string;
  default_reasoning_level: ReasoningLevel | null;
  parameter_overrides: ParameterOverrides;
  fallback_virtual_model_id: string | null;
  enabled: boolean;
}

interface AppConfig {
  providers: Provider[];
  upstream_models: UpstreamModel[];
  virtual_models: VirtualModel[];
}

interface ProxyStatus {
  state: "running" | "stopped";
  address: string | null;
}

interface ModelConnectionTestResult {
  success: boolean;
  durationMs: number;
  message: string;
}

interface ProviderCatalogModel {
  id: string;
  displayName: string;
}

interface IdeStatus {
  installed: boolean;
  compatible: boolean;
  ideRunning: boolean;

  state: "not_installed" | "vendor_original" | "patched" | "modified" | "incompatible";
  appPath: string;
  appVersion: string | null;
  extensionVersion: string | null;
  extensionSha256: string | null;
  message: string;
  integrationState: "disabled" | "enabled" | "external" | "conflict";
  settingsPath: string;
  integrationMessage: string;
  canEnableIntegration: boolean;
  canLaunchIde: boolean;
  canDisableIntegration: boolean;
}

const emptyParameters = (): ParameterOverrides => ({
  temperature: null,
  max_tokens: null,
  top_p: null,
  top_k: null,
  extra_body: null,
});

let config: AppConfig = {
  providers: [],
  upstream_models: [],
  virtual_models: [],
};
let latestProxyStatus: ProxyStatus | null = null;
let latestIdeStatus: IdeStatus | null = null;
let noticeTimer: number | null = null;
let editingProviderId: string | null = null;
let draftProviderId = `provider-${crypto.randomUUID()}`;
let catalogModels: ProviderCatalogModel[] = [];
let selectedCatalogModelIds = new Set<string>();

const element = <T extends HTMLElement>(selector: string): T => {
  const value = document.querySelector<T>(selector);
  if (!value) throw new Error(`Missing element: ${selector}`);
  return value;
};

const notice = element<HTMLDivElement>("#notice");
const noticeText = element<HTMLSpanElement>("#notice-text");
const providerList = element<HTMLDivElement>("#provider-list");
const providerCount = element<HTMLSpanElement>("#provider-count");
const providerForm = element<HTMLFormElement>("#provider-form");
const providerFormPanel = element<HTMLDetailsElement>("#provider-form-panel");
const openProviderFormButton = element<HTMLButtonElement>("#open-provider-form");
const catalogResults = element<HTMLElement>("#catalog-results");
const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
const saveProviderButton = element<HTMLButtonElement>("#save-provider");
const startProxyButton = element<HTMLButtonElement>("#start-proxy");
const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
const enableIdeIntegrationButton = element<HTMLButtonElement>("#enable-ide-integration");
const launchIdeButton = element<HTMLButtonElement>("#launch-ide");
const disableIdeIntegrationButton = element<HTMLButtonElement>("#disable-ide-integration");

providerFormPanel.open = false;

function showNotice(message: string, kind: "success" | "error" = "success"): void {
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeText.textContent = message;
  notice.className = `notice ${kind}`;
  notice.hidden = false;
  noticeTimer = window.setTimeout(() => {
    notice.hidden = true;
    noticeTimer = null;
  }, kind === "error" ? 8000 : 4000);
}

function dismissNotice(): void {
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeTimer = null;
  notice.hidden = true;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
}

function setReadinessStep(
  selector: string,
  valueSelector: string,
  state: "pending" | "ready" | "attention",
  value: string,
): void {
  element<HTMLLIElement>(selector).dataset.state = state;
  element<HTMLElement>(valueSelector).textContent = value;
}

function renderReadiness(): void {
  const modelCountValue = config.virtual_models.length;
  const proxyRunning = latestProxyStatus?.state === "running";
  const ideReady = latestIdeStatus
    ? latestIdeStatus.compatible
      && (latestIdeStatus.integrationState === "enabled" || latestIdeStatus.integrationState === "external")
    : false;

  setReadinessStep(
    "#readiness-models",
    "#readiness-models-value",
    modelCountValue > 0 ? "ready" : "attention",
    modelCountValue > 0 ? `${modelCountValue} 个可用` : "待添加",
  );
  setReadinessStep(
    "#readiness-proxy",
    "#readiness-proxy-value",
    latestProxyStatus === null ? "pending" : proxyRunning ? "ready" : "attention",
    latestProxyStatus === null ? "检查中" : proxyRunning ? "运行中" : "待启动",
  );
  setReadinessStep(
    "#readiness-ide",
    "#readiness-ide-value",
    latestIdeStatus === null ? "pending" : ideReady ? "ready" : "attention",
    latestIdeStatus === null ? "检查中" : ideReady ? "已接入" : "待启用",
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  if (modelCountValue === 0) {
    title.textContent = "先添加供应商并选择模型";
    detail.textContent = "连接代理、拉取模型目录，再选择要接入 IDE 的模型。";
  } else if (latestProxyStatus === null || latestIdeStatus === null) {
    title.textContent = "正在确认运行状态…";
    detail.textContent = "检查本地代理和 Antigravity IDE 接入状态。";
  } else if (!proxyRunning) {
    title.textContent = "模型已就绪，下一步启动代理";
    detail.textContent = "代理启动后，IDE 才能访问自定义模型。";
  } else if (!ideReady && latestIdeStatus.ideRunning) {
    title.textContent = "请先完全退出 Antigravity IDE";
    detail.textContent = "退出后刷新状态，再启用原生配置接入。";
  } else if (!ideReady) {
    title.textContent = "代理已就绪，下一步启用 IDE 接入";
    detail.textContent = "启用只会管理 jetski.cloudCodeUrl，不修改厂商 App。";
  } else if (latestIdeStatus.ideRunning) {
    title.textContent = "一切就绪，Antigravity IDE 正在运行";
    detail.textContent = "自定义模型请求将通过本地代理发送到你的 Provider。";
  } else {
    title.textContent = "接入已就绪，可以启动 Antigravity IDE";
    detail.textContent = "启动后即可在模型列表中选择自定义模型。";
  }
}

async function withBusy(button: HTMLButtonElement, action: () => Promise<void>): Promise<void> {
  const label = button.textContent;
  button.dataset.busy = "true";
  button.disabled = true;
  button.textContent = "处理中…";
  try {
    await action();
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    button.dataset.busy = "false";
    button.textContent = label;
    button.disabled = button.dataset.unavailable === "true";
  }
}

function renderProxy(status: ProxyStatus): void {
  latestProxyStatus = status;
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLElement>("#proxy-address");
  const running = status.state === "running";
  state.textContent = running ? "运行中" : "已停止";
  state.className = `status-pill ${running ? "success" : "neutral"}`;
  address.textContent = running && status.address ? status.address : "127.0.0.1:50999";
  startProxyButton.hidden = running;
  stopProxyButton.hidden = !running;
  setButtonUnavailable(startProxyButton, running);
  setButtonUnavailable(stopProxyButton, !running);
  renderReadiness();
}

function renderIde(status: IdeStatus): void {
  latestIdeStatus = status;
  const state = element<HTMLSpanElement>("#ide-state");
  const detail = element<HTMLParagraphElement>("#ide-detail");
  const labels: Record<IdeStatus["state"], string> = {
    not_installed: "未安装",
    vendor_original: "厂商原版",
    patched: "历史补丁",
    modified: "未知修改",
    incompatible: "不兼容",
  };
  state.textContent = labels[status.state];
  state.className = `status-pill ${status.state === "vendor_original" ? "success" : status.state === "patched" ? "accent" : "neutral"}`;
  const versions = status.appVersion
    ? `IDE ${status.appVersion} · Extension ${status.extensionVersion ?? "未知"}${status.ideRunning ? " · 正在运行" : ""}`
    : status.appPath;
  detail.textContent = `${versions} — ${status.message}`;

  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");
  const integrationLabels: Record<IdeStatus["integrationState"], string> = {
    disabled: "未启用",
    enabled: "已启用",
    external: "外部配置",
    conflict: "配置冲突",
  };
  integrationState.textContent = integrationLabels[status.integrationState];
  integrationState.className = `status-pill ${status.integrationState === "enabled" || status.integrationState === "external" ? "accent" : "neutral"}`;
  integrationDetail.textContent = status.integrationMessage;
  element<HTMLElement>("#ide-settings-path").textContent = status.settingsPath;

  const integrationReady = status.integrationState === "enabled" || status.integrationState === "external";
  enableIdeIntegrationButton.hidden = status.integrationState !== "disabled";
  launchIdeButton.hidden = !integrationReady || status.ideRunning;
  disableIdeIntegrationButton.hidden = status.integrationState !== "enabled";
  enableIdeIntegrationButton.textContent = status.ideRunning ? "退出 IDE 后启用" : "启用 IDE 接入";
  disableIdeIntegrationButton.textContent = status.ideRunning ? "退出 IDE 后停用" : "停用 IDE 接入";
  setButtonUnavailable(enableIdeIntegrationButton, !status.canEnableIntegration);
  setButtonUnavailable(launchIdeButton, !status.canLaunchIde);
  setButtonUnavailable(disableIdeIntegrationButton, !status.canDisableIntegration);
  renderReadiness();
}

function protocolName(protocol: ProviderProtocol): string {
  return { openai: "OpenAI", anthropic: "Anthropic", gemini: "Gemini" }[protocol];
}

function renderProviders(): void {
  providerCount.textContent = String(config.providers.length);
  providerList.replaceChildren();
  renderReadiness();

  if (config.providers.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "还没有供应商。添加代理连接后即可自动获取并选择模型。";
    providerList.append(empty);
    return;
  }

  for (const provider of config.providers) {
    const card = document.createElement("article");
    card.className = "provider-card";
    const heading = document.createElement("div");
    heading.className = "provider-card-heading";
    const identity = document.createElement("div");
    identity.className = "provider-identity";
    const title = document.createElement("h3");
    title.textContent = provider.name;
    const protocol = document.createElement("span");
    protocol.className = "status-pill neutral";
    protocol.textContent = protocolName(provider.protocol);
    const endpoint = document.createElement("code");
    endpoint.className = "provider-endpoint";
    endpoint.textContent = provider.models_endpoint;
    endpoint.title = provider.models_endpoint;
    identity.append(title, endpoint);

    const modelLinks = config.virtual_models.flatMap((virtualModel) => {
      const upstream = config.upstream_models.find(
        (item) => item.id === virtualModel.upstream_model_id && item.provider_id === provider.id,
      );
      return upstream ? [{ virtualModel, upstream }] : [];
    });
    const providerMeta = document.createElement("div");
    providerMeta.className = "provider-meta";
    const count = document.createElement("strong");
    count.textContent = `${modelLinks.length} 个模型`;
    providerMeta.append(protocol, count);
    heading.append(identity, providerMeta);

    const providerActions = document.createElement("div");
    providerActions.className = "provider-actions";
    const manage = document.createElement("button");
    manage.type = "button";
    manage.className = "secondary compact-button";
    manage.textContent = "管理模型";
    manage.addEventListener("click", () => openProviderEditor(provider.id));
    const removeProviderButton = document.createElement("button");
    removeProviderButton.type = "button";
    removeProviderButton.className = "danger-text";
    removeProviderButton.textContent = "删除供应商";
    armDestructiveButton(removeProviderButton, "确认删除", () => {
      void removeProvider(provider.id, removeProviderButton);
    });
    providerActions.append(manage, removeProviderButton);

    const models = document.createElement("div");
    models.className = "provider-models";
    if (modelLinks.length === 0) {
      const empty = document.createElement("p");
      empty.className = "provider-model-empty";
      empty.textContent = "尚未接入模型";
      models.append(empty);
    } else {
      for (const { virtualModel, upstream } of modelLinks) {
        models.append(providerModelRow(virtualModel, upstream));
      }
    }

    card.append(heading, providerActions, models);
    providerList.append(card);
  }
}

function providerModelRow(virtualModel: VirtualModel, upstream: UpstreamModel): HTMLElement {
  const row = document.createElement("div");
  row.className = "provider-model-row";
  const content = document.createElement("div");
  content.className = "model-card-content";
  const titleRow = document.createElement("div");
  titleRow.className = "model-title-row";
  const title = document.createElement("h4");
  title.textContent = virtualModel.display_name;
  const hostModelId = document.createElement("code");
  hostModelId.className = "host-model-id";
  hostModelId.textContent = effectiveHostModelId(virtualModel);
  titleRow.append(title, hostModelId);
  const meta = document.createElement("p");
  meta.className = "muted compact";
  meta.textContent = upstream.upstream_model_id;
  content.append(titleRow, meta);

  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("图片"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("工具"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("思考"));
  }
  content.append(capabilities);

  const connectionResult = document.createElement("p");
  connectionResult.className = "connection-result";
  connectionResult.setAttribute("role", "status");
  connectionResult.setAttribute("aria-live", "polite");
  connectionResult.hidden = true;
  content.append(connectionResult);

  const actions = document.createElement("div");
  actions.className = "model-card-actions";
  const testConnection = document.createElement("button");
  testConnection.type = "button";
  testConnection.className = "secondary compact-button";
  testConnection.textContent = "测试";
  testConnection.addEventListener("click", () => {
    void withBusy(testConnection, async () => {
      connectionResult.hidden = false;
      connectionResult.className = "connection-result pending";
      connectionResult.textContent = "正在发送最小请求…";
      try {
        const result = await invoke<ModelConnectionTestResult>("test_model_connection", {
          virtualModelId: virtualModel.id,
        });
        renderConnectionResult(connectionResult, result);
      } catch (error) {
        connectionResult.className = "connection-result error";
        connectionResult.textContent = `测试失败 · ${errorMessage(error)}`;
      }
    });
  });
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "danger-text";
  remove.textContent = "移除";
  armDestructiveButton(remove, "确认移除", () => {
    void removeModel(virtualModel.id, remove);
  });
  actions.append(testConnection, remove);
  row.append(content, actions);
  return row;
}

function renderConnectionResult(
  target: HTMLElement,
  result: ModelConnectionTestResult,
): void {
  target.className = `connection-result ${result.success ? "success" : "error"}`;
  target.textContent = result.success
    ? `连接正常 · ${result.durationMs} ms`
    : `连接失败 · ${result.message}`;
  target.title = result.message;
}

function armDestructiveButton(
  button: HTMLButtonElement,
  confirmLabel: string,
  action: () => void,
): void {
  const initialLabel = button.textContent ?? "删除";
  let armed = false;
  let resetTimer: number | null = null;
  button.addEventListener("click", () => {
    if (!armed) {
      armed = true;
      button.textContent = confirmLabel;
      button.classList.add("danger-confirm");
      resetTimer = window.setTimeout(() => {
        armed = false;
        button.textContent = initialLabel;
        button.classList.remove("danger-confirm");
      }, 4000);
      return;
    }
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    action();
  });
}

function capabilityBadge(label: string): HTMLSpanElement {
  const badge = document.createElement("span");
  badge.className = "capability-badge";
  badge.textContent = label;
  return badge;
}

function effectiveHostModelId(model: VirtualModel): string {
  if (model.host_model_id) return model.host_model_id;
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(model.id)) {
    hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  }
  return `MODEL_PLACEHOLDER_M${400 + (hash % 200)}`;
}

function nextHostModelId(occupied: Set<string>): string {
  for (let value = 400; value < 600; value += 1) {
    const candidate = `MODEL_PLACEHOLDER_M${value}`;
    if (!occupied.has(candidate)) {
      occupied.add(candidate);
      return candidate;
    }
  }
  throw new Error("IDE 自定义模型槽位已用完");
}

function reasoningLevels(protocol: ProviderProtocol): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  if (protocol === "anthropic") {
    return {
      low: { kind: "budget_tokens", value: 1024 },
      medium: { kind: "budget_tokens", value: 4096 },
      high: { kind: "budget_tokens", value: 8192 },
    };
  }
  if (protocol === "gemini") {
    return {
      low: { kind: "native_level", value: "low" },
      medium: { kind: "native_level", value: "medium" },
      high: { kind: "native_level", value: "high" },
    };
  }
  return {
    low: { kind: "effort", value: "low" },
    medium: { kind: "effort", value: "medium" },
    high: { kind: "effort", value: "high" },
  };
}

async function persistConfig(next: AppConfig): Promise<void> {
  config = await invoke<AppConfig>("save_config", { config: next });
  renderProviders();
}

async function removeModel(virtualModelId: string, button: HTMLButtonElement): Promise<void> {
  await withBusy(button, async () => {
    const target = config.virtual_models.find((item) => item.id === virtualModelId);
    if (!target) return;

    const remainingVirtualModels = config.virtual_models.filter((item) => item.id !== virtualModelId);
    const upstreamStillUsed = remainingVirtualModels.some(
      (item) => item.upstream_model_id === target.upstream_model_id,
    );
    const remainingUpstreamModels = upstreamStillUsed
      ? config.upstream_models
      : config.upstream_models.filter((item) => item.id !== target.upstream_model_id);

    await persistConfig({
      providers: config.providers,
      upstream_models: remainingUpstreamModels,
      virtual_models: remainingVirtualModels,
    });
    showNotice("模型已从 IDE 接入列表移除");
  });
}

async function removeProvider(providerId: string, button: HTMLButtonElement): Promise<void> {
  await withBusy(button, async () => {
    const upstreamIds = new Set(
      config.upstream_models
        .filter((item) => item.provider_id === providerId)
        .map((item) => item.id),
    );
    await persistConfig({
      providers: config.providers.filter((item) => item.id !== providerId),
      upstream_models: config.upstream_models.filter((item) => item.provider_id !== providerId),
      virtual_models: config.virtual_models.filter(
        (item) => !upstreamIds.has(item.upstream_model_id),
      ),
    });
    showNotice("供应商及其接入模型已删除");
  });
}

function suggestedEndpoints(
  baseUrl: string,
  protocol: ProviderProtocol,
): { modelsEndpoint: string; generateEndpoint: string } {
  const base = baseUrl.trim().replace(/\/+$/, "");
  if (!base) return { modelsEndpoint: "", generateEndpoint: "" };
  if (protocol === "gemini") {
    const apiBase = base.endsWith("/v1beta") ? base : `${base}/v1beta`;
    return {
      modelsEndpoint: `${apiBase}/models`,
      generateEndpoint: `${apiBase}/models/{model}:generateContent`,
    };
  }
  const apiBase = base.endsWith("/v1") ? base : `${base}/v1`;
  return {
    modelsEndpoint: `${apiBase}/models`,
    generateEndpoint: protocol === "anthropic"
      ? `${apiBase}/messages`
      : `${apiBase}/chat/completions`,
  };
}

function inferProviderBase(provider: Provider): string {
  const suffixes = [
    "/v1/chat/completions",
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

function updateSuggestedEndpoints(): void {
  const protocol = element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
  const baseUrl = element<HTMLInputElement>("#provider-base-url").value;
  const endpoints = suggestedEndpoints(baseUrl, protocol);
  element<HTMLInputElement>("#models-endpoint").value = endpoints.modelsEndpoint;
  element<HTMLInputElement>("#generate-endpoint").value = endpoints.generateEndpoint;
  resetCatalogResults();
}

function providerFromForm(): Provider {
  const protocol = element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
  const name = element<HTMLInputElement>("#provider-name").value.trim();
  const generateEndpoint = element<HTMLInputElement>("#generate-endpoint").value.trim();
  const modelsEndpoint = element<HTMLInputElement>("#models-endpoint").value.trim();
  const apiKey = element<HTMLInputElement>("#api-key").value;
  const existing = editingProviderId
    ? config.providers.find((item) => item.id === editingProviderId)
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

function resetCatalogResults(): void {
  catalogModels = [];
  selectedCatalogModelIds = new Set();
  catalogModelList.replaceChildren();
  catalogResults.hidden = true;
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  saveProviderButton.disabled = true;
}

function resetProviderEditor(): void {
  editingProviderId = null;
  draftProviderId = `provider-${crypto.randomUUID()}`;
  providerForm.reset();
  element<HTMLInputElement>("#tools").checked = true;
  element<HTMLButtonElement>("#toggle-api-key").textContent = "显示";
  element<HTMLInputElement>("#api-key").type = "password";
  element<HTMLElement>("#provider-form-title").textContent = "添加供应商";
  resetCatalogResults();
}

function openProviderEditor(providerId: string | null = null): void {
  resetProviderEditor();
  editingProviderId = providerId;
  const provider = providerId
    ? config.providers.find((item) => item.id === providerId)
    : undefined;
  if (provider) {
    draftProviderId = provider.id;
    element<HTMLElement>("#provider-form-title").textContent = `管理供应商 · ${provider.name}`;
    element<HTMLInputElement>("#provider-name").value = provider.name;
    element<HTMLSelectElement>("#protocol").value = provider.protocol;
    element<HTMLInputElement>("#provider-base-url").value = inferProviderBase(provider);
    element<HTMLInputElement>("#api-key").value = provider.api_key;
    element<HTMLInputElement>("#models-endpoint").value = provider.models_endpoint;
    element<HTMLInputElement>("#generate-endpoint").value = provider.generate_endpoint;
  }
  providerFormPanel.open = true;
  providerFormPanel.scrollIntoView({ behavior: "smooth", block: "start" });
  window.setTimeout(() => element<HTMLInputElement>("#provider-name").focus(), 250);
}

async function fetchProviderCatalog(): Promise<void> {
  if (!providerForm.reportValidity()) return;
  const provider = providerFromForm();
  const fetched = await invoke<ProviderCatalogModel[]>("fetch_provider_catalog", { provider });
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const existingUpstreams = editingProviderId
    ? config.upstream_models.filter((item) => item.provider_id === editingProviderId)
    : [];
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
  catalogResults.hidden = false;
  element<HTMLElement>("#catalog-status").textContent = `连接正常 · 获取到 ${fetched.length} 个模型`;
  renderCatalogModels();
  catalogResults.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function renderCatalogModels(): void {
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    row.className = "catalog-model-row";
    const select = document.createElement("label");
    select.className = "catalog-model-select";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selectedCatalogModelIds.has(model.id);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedCatalogModelIds.add(model.id);
      else selectedCatalogModelIds.delete(model.id);
      updateCatalogSelection();
    });
    const copy = document.createElement("span");
    const name = document.createElement("strong");
    name.textContent = model.displayName;
    const id = document.createElement("code");
    id.textContent = model.id;
    copy.append(name, id);
    select.append(checkbox, copy);

    const test = document.createElement("button");
    test.type = "button";
    test.className = "secondary compact-button";
    test.textContent = "预检";
    const result = document.createElement("span");
    result.className = "catalog-model-test-result";
    result.setAttribute("role", "status");
    test.addEventListener("click", () => {
      void withBusy(test, async () => {
        result.className = "catalog-model-test-result pending";
        result.textContent = "测试中";
        const response = await invoke<ModelConnectionTestResult>(
          "test_provider_model_connection",
          { provider: providerFromForm(), upstreamModelId: model.id },
        );
        result.className = `catalog-model-test-result ${response.success ? "success" : "error"}`;
        result.textContent = response.success
          ? `${response.durationMs} ms`
          : response.message;
        result.title = response.message;
      });
    });
    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.append(result, test);
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

function updateCatalogSelection(): void {
  const count = selectedCatalogModelIds.size;
  element<HTMLElement>("#selected-model-count").textContent =
    count > 0 ? `已选择 ${count} 个模型` : "尚未选择模型";
  saveProviderButton.disabled = count === 0;
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

async function saveProvider(): Promise<void> {
  if (!providerForm.reportValidity() || selectedCatalogModelIds.size === 0) return;
  const provider = providerFromForm();
  const providerUpstreams = config.upstream_models.filter(
    (item) => item.provider_id === provider.id,
  );
  const providerUpstreamIds = new Set(providerUpstreams.map((item) => item.id));
  const remainingUpstreams = config.upstream_models.filter(
    (item) => item.provider_id !== provider.id,
  );
  const remainingVirtuals = config.virtual_models.filter(
    (item) => !providerUpstreamIds.has(item.upstream_model_id),
  );
  const occupiedHostModelIds = new Set(remainingVirtuals.map(effectiveHostModelId));
  const selectedModels = catalogModels.filter((model) =>
    selectedCatalogModelIds.has(model.id)
  );
  if (selectedModels.length === 0) {
    showNotice("当前模型目录中没有有效选项，请重新获取模型", "error");
    return;
  }
  const protocol = provider.protocol;
  const reasoningEnabled = element<HTMLInputElement>("#reasoning").checked;
  const nextUpstreams: UpstreamModel[] = [];
  const nextVirtuals: VirtualModel[] = [];

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find(
      (item) => item.upstream_model_id === model.id,
    );
    if (existingUpstream) {
      nextUpstreams.push(existingUpstream);
      const existingVirtuals = config.virtual_models.filter(
        (item) => item.upstream_model_id === existingUpstream.id,
      );
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
        nextVirtuals.push(virtualModel);
      }
      if (existingVirtuals.length > 0) continue;
    }

    const id = crypto.randomUUID();
    const upstreamId = existingUpstream?.id ?? `upstream-${id}`;
    if (!existingUpstream) {
      nextUpstreams.push({
        id: upstreamId,
        provider_id: provider.id,
        upstream_model_id: model.id,
        display_name: model.displayName,
        capabilities: {
          vision: element<HTMLInputElement>("#vision").checked,
          tools: element<HTMLInputElement>("#tools").checked,
          reasoning: { levels: reasoningEnabled ? reasoningLevels(protocol) : {} },
        },
        parameter_overrides: emptyParameters(),
        enabled: true,
      });
    }
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
  }

  const providers = editingProviderId
    ? config.providers.map((item) => item.id === provider.id ? provider : item)
    : [...config.providers, provider];
  await persistConfig({
    providers,
    upstream_models: [...remainingUpstreams, ...nextUpstreams],
    virtual_models: [...remainingVirtuals, ...nextVirtuals],
  });
  const action = editingProviderId ? "已更新" : "已添加";
  resetProviderEditor();
  providerFormPanel.open = false;
  showNotice(`${action}供应商 ${provider.name}，接入 ${selectedModels.length} 个模型`);
}

async function refreshProxy(): Promise<void> {
  renderProxy(await invoke<ProxyStatus>("proxy_status"));
}

async function refreshIde(): Promise<void> {
  renderIde(await invoke<IdeStatus>("discover_ide"));
}

async function initialize(): Promise<void> {
  try {
    config = await invoke<AppConfig>("get_config");
    renderProviders();
    await Promise.all([refreshProxy(), refreshIde()]);
  } catch (error) {
    showNotice(`初始化失败：${errorMessage(error)}`, "error");
  }
}

startProxyButton.addEventListener("click", () => void withBusy(startProxyButton, async () => {
  renderProxy(await invoke<ProxyStatus>("start_proxy"));
  showNotice("本地代理已启动");
}));

stopProxyButton.addEventListener("click", () => void withBusy(stopProxyButton, async () => {
  renderProxy(await invoke<ProxyStatus>("stop_proxy"));
  showNotice("本地代理已停止");
}));

element<HTMLButtonElement>("#refresh-ide").addEventListener("click", (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  void withBusy(button, refreshIde);
});

element<HTMLButtonElement>("#dismiss-notice").addEventListener("click", dismissNotice);

openProviderFormButton.addEventListener("click", () => openProviderEditor());

element<HTMLButtonElement>("#cancel-provider").addEventListener("click", () => {
  resetProviderEditor();
  providerFormPanel.open = false;
});

enableIdeIntegrationButton.addEventListener("click", () => {
  void withBusy(enableIdeIntegrationButton, async () => {
    showNotice("正在启用 IDE 原生配置接入…");
    renderIde(await invoke<IdeStatus>("enable_ide_integration"));
    showNotice("IDE 原生配置接入已启用；请启动代理后打开 Antigravity IDE");
  });
});

launchIdeButton.addEventListener("click", () => void withBusy(launchIdeButton, async () => {
  await invoke<void>("launch_ide");
  showNotice("已启动厂商原版 Antigravity IDE");
}));

disableIdeIntegrationButton.addEventListener("click", () => {
  void withBusy(disableIdeIntegrationButton, async () => {
    showNotice("正在停用 IDE 原生配置接入并恢复原 settings…");
    renderIde(await invoke<IdeStatus>("disable_ide_integration"));
    showNotice("IDE 原生配置接入已停用，原 settings 已恢复");
  });
});

providerForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void withBusy(saveProviderButton, saveProvider);
});

element<HTMLButtonElement>("#fetch-provider-models").addEventListener("click", (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  void withBusy(button, fetchProviderCatalog);
});

element<HTMLInputElement>("#provider-base-url").addEventListener("input", updateSuggestedEndpoints);
element<HTMLSelectElement>("#protocol").addEventListener("change", updateSuggestedEndpoints);
for (const selector of ["#models-endpoint", "#generate-endpoint", "#api-key"]) {
  element<HTMLInputElement>(selector).addEventListener("input", resetCatalogResults);
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
  renderCatalogModels();
});

element<HTMLButtonElement>("#toggle-api-key").addEventListener("click", (event) => {
  const input = element<HTMLInputElement>("#api-key");
  const button = event.currentTarget as HTMLButtonElement;
  const visible = input.type === "text";
  input.type = visible ? "password" : "text";
  button.textContent = visible ? "显示" : "隐藏";
});

void initialize();
