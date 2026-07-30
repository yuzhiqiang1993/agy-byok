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

interface IdeStatus {
  installed: boolean;
  compatible: boolean;
  canDryRun: boolean;

  canRestore: boolean;
  receiptPath: string | null;
  state: "not_installed" | "vendor_original" | "patched" | "modified" | "incompatible";
  appPath: string;
  appVersion: string | null;
  extensionVersion: string | null;
  extensionSha256: string | null;
  message: string;
  managedState: "not_created" | "ready" | "invalid";
  managedAppPath: string;
  managedReceiptPath: string | null;
  managedMessage: string;
  canCreateManaged: boolean;
  canLaunchManaged: boolean;
  canRemoveManaged: boolean;
}

interface DryRunResult {
  profileId: string;
  endpoint: string;
  candidateSha256: string;
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

const element = <T extends HTMLElement>(selector: string): T => {
  const value = document.querySelector<T>(selector);
  if (!value) throw new Error(`Missing element: ${selector}`);
  return value;
};

const notice = element<HTMLDivElement>("#notice");
const modelList = element<HTMLDivElement>("#model-list");
const modelCount = element<HTMLSpanElement>("#model-count");
const modelForm = element<HTMLFormElement>("#model-form");
const startProxyButton = element<HTMLButtonElement>("#start-proxy");
const stopProxyButton = element<HTMLButtonElement>("#stop-proxy");
const dryRunButton = element<HTMLButtonElement>("#dry-run-ide");
const createManagedIdeButton = element<HTMLButtonElement>("#create-managed-ide");
const launchManagedIdeButton = element<HTMLButtonElement>("#launch-managed-ide");
const removeManagedIdeButton = element<HTMLButtonElement>("#remove-managed-ide");
const restoreIdeButton = element<HTMLButtonElement>("#restore-ide");

function showNotice(message: string, kind: "success" | "error" = "success"): void {
  notice.textContent = message;
  notice.className = `notice ${kind}`;
  notice.hidden = false;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
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
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLParagraphElement>("#proxy-address");
  const running = status.state === "running";
  state.textContent = running ? "运行中" : "已停止";
  state.className = `status-pill ${running ? "success" : "neutral"}`;
  address.textContent = running && status.address ? status.address : "127.0.0.1:50999";
  setButtonUnavailable(startProxyButton, running);
  setButtonUnavailable(stopProxyButton, !running);
}

function renderIde(status: IdeStatus): void {
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
    ? `IDE ${status.appVersion} · Extension ${status.extensionVersion ?? "未知"}`
    : status.appPath;
  detail.textContent = `${versions} — ${status.message}`;

  const managedState = element<HTMLSpanElement>("#managed-ide-state");
  const managedDetail = element<HTMLParagraphElement>("#managed-ide-detail");
  const managedLabels: Record<IdeStatus["managedState"], string> = {
    not_created: "未创建",
    ready: "已就绪",
    invalid: "状态异常",
  };
  managedState.textContent = managedLabels[status.managedState];
  managedState.className = `status-pill ${status.managedState === "ready" ? "accent" : "neutral"}`;
  managedDetail.textContent = `${status.managedAppPath} — ${status.managedMessage}`;

  setButtonUnavailable(dryRunButton, !status.canDryRun);
  setButtonUnavailable(createManagedIdeButton, !status.canCreateManaged);
  setButtonUnavailable(launchManagedIdeButton, !status.canLaunchManaged);
  setButtonUnavailable(removeManagedIdeButton, !status.canRemoveManaged);
  setButtonUnavailable(restoreIdeButton, !status.canRestore);
}

function protocolName(protocol: ProviderProtocol): string {
  return { openai: "OpenAI", anthropic: "Anthropic", gemini: "Gemini" }[protocol];
}

function renderModels(): void {
  modelCount.textContent = String(config.virtual_models.length);
  modelList.replaceChildren();

  if (config.virtual_models.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "还没有自定义模型。使用下方表单添加第一个模型。";
    modelList.append(empty);
    return;
  }

  for (const virtualModel of config.virtual_models) {
    const upstream = config.upstream_models.find((item) => item.id === virtualModel.upstream_model_id);
    const provider = upstream
      ? config.providers.find((item) => item.id === upstream.provider_id)
      : undefined;

    const card = document.createElement("article");
    card.className = "model-card";

    const content = document.createElement("div");
    const title = document.createElement("h3");
    title.textContent = virtualModel.display_name;
    const meta = document.createElement("p");
    meta.className = "muted compact";
    meta.textContent = provider && upstream
      ? `${protocolName(provider.protocol)} · ${upstream.upstream_model_id} · ${provider.generate_endpoint}`
      : "配置引用不完整";
    content.append(title, meta);

    const capabilities = document.createElement("div");
    capabilities.className = "capability-list";
    if (upstream?.capabilities.vision) capabilities.append(capabilityBadge("图片"));
    if (upstream?.capabilities.tools) capabilities.append(capabilityBadge("工具"));
    if (upstream && Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
      capabilities.append(capabilityBadge("思考"));
    }
    content.append(capabilities);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "danger-text";
    remove.textContent = "删除";
    remove.addEventListener("click", () => void removeModel(virtualModel.id, remove));

    card.append(content, remove);
    modelList.append(card);
  }
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

function nextHostModelId(): string {
  const occupied = new Set(config.virtual_models.map(effectiveHostModelId));
  for (let value = 400; value < 600; value += 1) {
    const candidate = `MODEL_PLACEHOLDER_M${value}`;
    if (!occupied.has(candidate)) return candidate;
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
  renderModels();
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
    const removedUpstream = config.upstream_models.find((item) => item.id === target.upstream_model_id);
    const providerStillUsed = removedUpstream
      ? remainingUpstreamModels.some((item) => item.provider_id === removedUpstream.provider_id)
      : true;
    const remainingProviders = removedUpstream && !providerStillUsed
      ? config.providers.filter((item) => item.id !== removedUpstream.provider_id)
      : config.providers;

    await persistConfig({
      providers: remainingProviders,
      upstream_models: remainingUpstreamModels,
      virtual_models: remainingVirtualModels,
    });
    showNotice("模型已删除");
  });
}

async function addModel(): Promise<void> {
  const protocol = element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
  const displayName = element<HTMLInputElement>("#display-name").value.trim();
  const generateEndpoint = element<HTMLInputElement>("#generate-endpoint").value.trim();
  const modelsEndpoint = element<HTMLInputElement>("#models-endpoint").value.trim();
  const upstreamModelId = element<HTMLInputElement>("#upstream-model-id").value.trim();
  const apiKey = element<HTMLInputElement>("#api-key").value;
  const reasoningEnabled = element<HTMLInputElement>("#reasoning").checked;
  const id = crypto.randomUUID();
  const providerId = `provider-${id}`;
  const upstreamId = `upstream-${id}`;
  const virtualId = `custom-${id}`;

  const provider: Provider = {
    id: providerId,
    name: displayName,
    protocol,
    models_endpoint: modelsEndpoint,
    generate_endpoint: generateEndpoint,
    api_key: apiKey,
    headers: {},
    default_parameters: emptyParameters(),
    connect_timeout_ms: 5000,
    request_timeout_ms: 120000,
    stream_idle_timeout_ms: 30000,
    enabled: true,
  };
  const upstream: UpstreamModel = {
    id: upstreamId,
    provider_id: providerId,
    upstream_model_id: upstreamModelId,
    display_name: displayName,
    capabilities: {
      vision: element<HTMLInputElement>("#vision").checked,
      tools: element<HTMLInputElement>("#tools").checked,
      reasoning: { levels: reasoningEnabled ? reasoningLevels(protocol) : {} },
    },
    parameter_overrides: emptyParameters(),
    enabled: true,
  };
  const virtualModel: VirtualModel = {
    id: virtualId,
    host_model_id: nextHostModelId(),
    upstream_model_id: upstreamId,
    display_name: displayName,
    default_reasoning_level: null,
    parameter_overrides: emptyParameters(),
    fallback_virtual_model_id: null,
    enabled: true,
  };

  await persistConfig({
    providers: [...config.providers, provider],
    upstream_models: [...config.upstream_models, upstream],
    virtual_models: [...config.virtual_models, virtualModel],
  });
  modelForm.reset();
  element<HTMLInputElement>("#tools").checked = true;
  showNotice(`已添加模型：${displayName}`);
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
    renderModels();
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

dryRunButton.addEventListener("click", () => void withBusy(dryRunButton, async () => {
  const result = await invoke<DryRunResult>("dry_run_ide");
  showNotice(`候选校验通过：${result.profileId}，哈希 ${result.candidateSha256}`);
  await refreshIde();
}));

createManagedIdeButton.addEventListener("click", () => {
  if (!window.confirm("将从 Google 厂商原版创建独立托管副本，并只对副本应用 Endpoint 补丁、ad-hoc 签名和必要的 quarantine 清理。厂商原版不会被修改。继续？")) return;
  void withBusy(createManagedIdeButton, async () => {
    renderIde(await invoke<IdeStatus>("create_managed_ide"));
    showNotice("托管 IDE 已创建；请先启动代理，再打开托管 IDE");
  });
});

launchManagedIdeButton.addEventListener("click", () => void withBusy(launchManagedIdeButton, async () => {
  await invoke<void>("launch_managed_ide");
  showNotice("已启动 AGY BYOK 托管 IDE");
}));

removeManagedIdeButton.addEventListener("click", () => {
  if (!window.confirm("删除前必须退出托管 IDE。该操作只删除 AGY BYOK 托管副本，不会修改厂商原版。继续？")) return;
  void withBusy(removeManagedIdeButton, async () => {
    renderIde(await invoke<IdeStatus>("remove_managed_ide"));
    showNotice("托管 IDE 已删除，厂商原版保持不变");
  });
});

restoreIdeButton.addEventListener("click", () => {
  if (!window.confirm("恢复前必须退出 Antigravity IDE。继续恢复历史事务保存的厂商原始文件并验证 Google 签名？")) return;
  void withBusy(restoreIdeButton, async () => {
    renderIde(await invoke<IdeStatus>("restore_ide"));
    showNotice("Antigravity IDE 已恢复到应用前状态");
  });
});

modelForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const button = element<HTMLButtonElement>("#save-model");
  void withBusy(button, addModel);
});

element<HTMLButtonElement>("#toggle-api-key").addEventListener("click", (event) => {
  const input = element<HTMLInputElement>("#api-key");
  const button = event.currentTarget as HTMLButtonElement;
  const visible = input.type === "text";
  input.type = visible ? "password" : "text";
  button.textContent = visible ? "显示" : "隐藏";
});

void initialize();
