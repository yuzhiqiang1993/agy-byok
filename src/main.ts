import { invoke } from "@tauri-apps/api/core";

type ProviderProtocol = "openai" | "anthropic" | "gemini";
type ReasoningLevel = "off" | "low" | "medium" | "high" | "x_high" | "max" | "auto";
type ConfigurableReasoningLevel = "low" | "medium" | "high" | "x_high" | "max";
type ReasoningVariant = "default" | ConfigurableReasoningLevel;

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
  proxy_port: number;
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

type ModelConnectionTestOutcome =
  | { kind: "result"; result: ModelConnectionTestResult }
  | { kind: "error"; message: string };

type ConnectionTestViewState =
  | { status: "testing"; message: string }
  | { status: "success"; message: string; durationMs: number }
  | { status: "error"; message: string };

interface ProviderTestSession {
  targetVirtualModelIds: string[];
  completedAt: number;
}

interface ProviderChangeSummary {
  addedUpstreamIds: string[];
  removedUpstreamIds: string[];
  addedVirtualModels: VirtualModel[];
  removedVirtualModels: VirtualModel[];
  retainedVirtualCount: number;
  legacyModelIds: string[];
  fallbackBlockers: string[];
}

interface ProviderSavePlan {
  provider: Provider;
  nextConfig: AppConfig;
  summary: ProviderChangeSummary;
  wasEditing: boolean;
}

interface ActivityItem {
  id: string;
  timestampMs: number;
  requestedVirtualModelId: string;
  virtualModelId: string;
  upstreamModelId: string | null;
  providerId: string;
  providerProtocol: string | null;
  statusCode: number;
  durationMs: number;
  errorCategory: string | null;
  errorDetail: string | null;
  stream: boolean;
  messageCount: number;
  toolCount: number;
  usedFallback: boolean;
  fallbackAttempted: boolean;
  fallbackSucceeded: boolean;
  promptTokens: number | null;
  completionTokens: number | null;
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
  integrationState: "disabled" | "enabled" | "external";
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
  proxy_port: 51234,
  providers: [],
  upstream_models: [],
  virtual_models: [],
};
let latestProxyStatus: ProxyStatus | null = null;
let latestIdeStatus: IdeStatus | null = null;
let proxyStatusLoadFailed = false;
let ideStatusLoadFailed = false;
let noticeTimer: number | null = null;
let editingProviderId: string | null = null;
let draftProviderId = `provider-${crypto.randomUUID()}`;
let catalogModels: ProviderCatalogModel[] = [];
let selectedCatalogModelIds = new Set<string>();
let globalCatalogReasoningVariants = new Set<ReasoningVariant>(["default"]);
let catalogReasoningVariantsByModel = new Map<string, Set<ReasoningVariant>>();
let catalogVisionEnabledModelIds = new Set<string>();
let catalogToolsEnabledModelIds = new Set<string>();
let catalogReasoningEnabledModelIds = new Set<string>();
let changedCatalogCapabilityModelIds = new Set<string>();
let changedCatalogReasoningModelIds = new Set<string>();
let legacyCatalogModelIds = new Set<string>();
let providerEditorDirty = false;
let providerEditorBusy = false;
let configMutationInProgress = false;
let pendingProviderSavePlan: ProviderSavePlan | null = null;
const connectionTestsInFlight = new Map<string, Promise<ModelConnectionTestOutcome>>();
const connectionTestResults = new Map<string, ConnectionTestViewState>();
const providerTestSessions = new Map<string, ProviderTestSession>();
const connectionTestWaiters: Array<() => void> = [];
let activeConnectionTests = 0;
let activityRequestVersion = 0;
let activityActionInProgress = false;
let activityRefreshInFlight: Promise<void> | null = null;
let activityItems: ActivityItem[] = [];
let activitySnapshot = "";
let activityFailedOnly = false;

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
const activityList = element<HTMLDivElement>("#activity-list");
const activityCount = element<HTMLSpanElement>("#activity-count");
const refreshActivityButton = element<HTMLButtonElement>("#refresh-activity");
const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
const readinessActionButton = element<HTMLButtonElement>("#readiness-action");
const providerDirtyBadge = element<HTMLElement>("#provider-editor-dirty");
const providerChangeSummary = element<HTMLElement>("#provider-change-summary");
const applyReasoningTemplateButton = element<HTMLButtonElement>("#apply-reasoning-template");
const failedActivityOnlyCheckbox = element<HTMLInputElement>("#activity-failed-only");

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
  const upstreamCountValue = config.upstream_models.length;
  const proxyRunning = latestProxyStatus?.state === "running";
  const ideReady = latestIdeStatus
    ? latestIdeStatus.compatible
      && latestIdeStatus.integrationState !== "disabled"
    : false;

  setReadinessStep(
    "#readiness-models",
    "#readiness-models-value",
    modelCountValue > 0 ? "ready" : "attention",
    modelCountValue > 0 ? `${modelCountValue} 个入口` : "待添加",
  );
  setReadinessStep(
    "#readiness-proxy",
    "#readiness-proxy-value",
    proxyStatusLoadFailed ? "attention" : latestProxyStatus === null ? "pending" : proxyRunning ? "ready" : "attention",
    proxyStatusLoadFailed ? "读取失败" : latestProxyStatus === null ? "检查中" : proxyRunning ? "运行中" : "待启动",
  );
  setReadinessStep(
    "#readiness-ide",
    "#readiness-ide-value",
    ideStatusLoadFailed ? "attention" : latestIdeStatus === null ? "pending" : ideReady ? "ready" : "attention",
    ideStatusLoadFailed
      ? "读取失败"
      : latestIdeStatus === null
        ? "检查中"
      : ideReady
        ? latestIdeStatus.integrationState === "external" ? "外部接入" : "已接入"
        : "待启用",
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  readinessActionButton.hidden = false;
  readinessActionButton.onclick = null;
  if (modelCountValue === 0) {
    title.textContent = "先添加上游服务并选择模型";
    detail.textContent = "读取上游模型目录，再选择要接入 IDE 的模型。";
    readinessActionButton.textContent = "添加第一个上游服务";
    readinessActionButton.onclick = () => openProviderEditor();
  } else if (proxyStatusLoadFailed || ideStatusLoadFailed) {
    title.textContent = "部分运行状态读取失败";
    detail.textContent = "请使用对应卡片的刷新操作重试。";
    readinessActionButton.hidden = true;
  } else if (latestProxyStatus === null || latestIdeStatus === null) {
    title.textContent = "正在确认运行状态…";
    detail.textContent = `已配置 ${upstreamCountValue} 个上游模型、${modelCountValue} 个 IDE 入口。`;
    readinessActionButton.hidden = true;
  } else if (!proxyRunning) {
    title.textContent = "模型已就绪，下一步启动代理";
    detail.textContent = "代理启动后，IDE 才能访问自定义模型。";
    readinessActionButton.textContent = "启动本地代理";
    readinessActionButton.onclick = () => startProxyButton.click();
  } else if (!ideReady) {
    title.textContent = "代理已就绪，下一步启用 IDE 接入";
    detail.textContent = latestIdeStatus.ideRunning
      ? "启用时应用会安全更新用户设置并自动重启 IDE。"
      : "启用只会管理 jetski.cloudCodeUrl，不修改厂商 App。";
    readinessActionButton.textContent = latestIdeStatus.ideRunning ? "启用并重启 IDE" : "启用 IDE 接入";
    readinessActionButton.onclick = () => enableIdeIntegrationButton.click();
  } else if (latestIdeStatus.ideRunning) {
    title.textContent = "一切就绪，Antigravity IDE 正在运行";
    detail.textContent = "IDE 模型请求将通过本地代理发送到你的上游服务。";
    readinessActionButton.hidden = true;
  } else {
    title.textContent = "接入已就绪，可以启动 Antigravity IDE";
    detail.textContent = "启动后即可在模型列表中选择自定义模型。";
    readinessActionButton.textContent = "启动 Antigravity IDE";
    readinessActionButton.onclick = () => launchIdeButton.click();
  }
}

async function withBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = "处理中…",
): Promise<void> {
  if (button.dataset.busy === "true") return;
  const label = button.textContent;
  button.dataset.busy = "true";
  button.disabled = true;
  button.textContent = busyLabel;
  try {
    await action();
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    button.dataset.busy = "false";
    button.textContent = label;
    button.disabled = button.dataset.unavailable === "true"
      || button.dataset.bulkBusy === "true";
  }
}

function invalidatePendingProviderSave(): void {
  pendingProviderSavePlan = null;
  providerChangeSummary.hidden = true;
  providerChangeSummary.className = "provider-change-summary";
}

function setProviderEditorDirty(dirty: boolean): void {
  providerEditorDirty = dirty;
  providerDirtyBadge.hidden = !dirty;
  if (dirty) invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

function refreshProviderEditorControls(): void {
  const hasSelection = selectedCatalogModelIds.size > 0;
  saveProviderButton.disabled = providerEditorBusy || !providerEditorDirty || !hasSelection;
  if (!providerEditorBusy) {
    saveProviderButton.textContent = pendingProviderSavePlan
      ? `确认保存并移除 ${pendingProviderSavePlan.summary.removedVirtualModels.length} 个 IDE 入口`
      : "保存上游服务";
  }
  openProviderFormButton.disabled = providerEditorBusy;
}

function setProviderEditorBusy(busy: boolean): void {
  providerEditorBusy = busy;
  providerForm.toggleAttribute("inert", busy);
  providerForm.setAttribute("aria-busy", String(busy));
  providerList.toggleAttribute("inert", busy);
  providerFormPanel.dataset.busy = String(busy);
  refreshProviderEditorControls();
}

async function withProviderEditorBusy(
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

function confirmDiscardProviderChanges(): boolean {
  if (providerEditorBusy) {
    showNotice("上游服务配置正在处理中，请稍候", "error");
    return false;
  }
  return !providerEditorDirty || window.confirm("当前有未保存的上游服务修改，确定放弃吗？");
}

function renderProxy(status: ProxyStatus): void {
  latestProxyStatus = status;
  proxyStatusLoadFailed = false;
  const actualPort = proxyPortFromAddress(status.address);
  if (actualPort !== null) config.proxy_port = actualPort;
  const state = element<HTMLSpanElement>("#proxy-state");
  const address = element<HTMLElement>("#proxy-address");
  const running = status.state === "running";
  state.textContent = running ? "运行中" : "已停止";
  state.className = `status-pill ${running ? "success" : "neutral"}`;
  address.textContent = status.address ?? `127.0.0.1:${config.proxy_port}`;
  startProxyButton.hidden = running;
  stopProxyButton.hidden = !running;
  setButtonUnavailable(startProxyButton, running);
  setButtonUnavailable(stopProxyButton, !running);
  renderReadiness();
}

function proxyPortFromAddress(address: string | null): number | null {
  if (!address) return null;
  const separator = address.lastIndexOf(":");
  const port = Number(address.slice(separator + 1));
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

function renderIde(status: IdeStatus): void {
  latestIdeStatus = status;
  ideStatusLoadFailed = false;
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
  };
  integrationState.textContent = integrationLabels[status.integrationState];
  integrationState.className = `status-pill ${status.integrationState === "enabled" ? "accent" : "neutral"}`;
  integrationDetail.textContent = status.integrationMessage;
  element<HTMLElement>("#ide-settings-path").textContent = status.settingsPath;

  const integrationReady = status.integrationState !== "disabled";
  enableIdeIntegrationButton.hidden = status.integrationState !== "disabled";
  launchIdeButton.hidden = !integrationReady || status.ideRunning;
  disableIdeIntegrationButton.hidden = !status.canDisableIntegration;
  enableIdeIntegrationButton.textContent = status.ideRunning ? "启用并重启 IDE" : "启用 IDE 接入";
  disableIdeIntegrationButton.textContent = status.ideRunning ? "停用并重启 IDE" : "停用 IDE 接入";
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

    const providerUpstreams = config.upstream_models.filter(
      (upstream) => upstream.provider_id === provider.id,
    );
    const modelLinks = config.virtual_models.flatMap((virtualModel) => {
      const upstream = providerUpstreams.find(
        (item) => item.id === virtualModel.upstream_model_id,
      );
      return upstream ? [{ virtualModel, upstream }] : [];
    });
    const providerMeta = document.createElement("div");
    providerMeta.className = "provider-meta";
    const count = document.createElement("strong");
    count.textContent = `${providerUpstreams.length} 个上游模型 · ${modelLinks.length} 个 IDE 入口`;
    providerMeta.append(protocol, count);
    heading.append(identity, providerMeta);

    const providerActions = document.createElement("div");
    providerActions.className = "provider-actions";
    const manage = document.createElement("button");
    manage.type = "button";
    manage.className = "secondary compact-button";
    manage.textContent = "管理上游模型";
    manage.addEventListener("click", () => openProviderEditor(provider.id));
    const removeProviderButton = document.createElement("button");
    removeProviderButton.type = "button";
    removeProviderButton.className = "danger-text";
    removeProviderButton.textContent = "删除上游服务";
    armDestructiveButton(
      removeProviderButton,
      `确认删除及 ${modelLinks.length} 个入口`,
      () => removeProvider(provider.id, removeProviderButton),
      () => destructiveMutationBlocker(new Set(modelLinks.map(({ virtualModel }) => virtualModel.id))),
    );
    const testAllModels = document.createElement("button");
    testAllModels.type = "button";
    testAllModels.className = "secondary compact-button provider-bulk-test";
    const allVirtualModels = modelLinks.map(({ virtualModel }) => virtualModel);
    const failedVirtualModels = allVirtualModels.filter(
      (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "error",
    );
    const currentVirtualIds = allVirtualModels.map((model) => model.id).sort();
    const storedTestSession = providerTestSessions.get(provider.id);
    const testSession = storedTestSession
      && JSON.stringify([...storedTestSession.targetVirtualModelIds].sort()) === JSON.stringify(currentVirtualIds)
      ? storedTestSession
      : undefined;
    testAllModels.textContent = testSession
      ? failedVirtualModels.length > 0
        ? `重试失败（${failedVirtualModels.length}）`
        : "重新测试全部"
      : "测试全部 IDE 入口";
    testAllModels.title = "所有上游服务共享最多 3 个并发测试";
    testAllModels.disabled = modelLinks.length === 0;
    testAllModels.addEventListener("click", () => {
      const currentFailures = allVirtualModels.filter(
        (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "error",
      );
      const targets = testSession && currentFailures.length > 0
        ? currentFailures
        : allVirtualModels;
      void withBusy(
        testAllModels,
        () => testProviderModels(
          provider.id,
          card,
          targets,
          currentVirtualIds,
          testAllModels,
        ),
        "准备测试…",
      );
    });
    const testSummary = document.createElement("span");
    testSummary.className = "provider-test-summary";
    if (testSession) {
      const passed = allVirtualModels.filter(
        (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "success",
      ).length;
      const time = formatActivityTime(testSession.completedAt).label;
      testSummary.classList.add(failedVirtualModels.length > 0 ? "error" : "success");
      testSummary.textContent = `最近测试 ${time} · ${passed} 通过 · ${failedVirtualModels.length} 失败`;
      providerActions.append(manage, removeProviderButton, testSummary, testAllModels);
    } else {
      providerActions.append(manage, removeProviderButton, testAllModels);
    }

    const models = document.createElement("div");
    models.className = "provider-models";
    if (modelLinks.length === 0) {
      const empty = document.createElement("p");
      empty.className = "provider-model-empty";
      empty.textContent = "尚未接入 IDE 模型入口";
      models.append(empty);
    } else {
      for (const upstream of providerUpstreams) {
        const virtualModels = modelLinks
          .filter((link) => link.upstream.id === upstream.id)
          .map((link) => link.virtualModel);
        if (virtualModels.length > 0) {
          models.append(providerModelGroup(provider, upstream, virtualModels));
        }
      }
    }

    card.append(heading, providerActions, models);
    providerList.append(card);
  }
}

function providerModelGroup(
  provider: Provider,
  upstream: UpstreamModel,
  virtualModels: VirtualModel[],
): HTMLElement {
  const group = document.createElement("details");
  group.className = "provider-model-group";
  const summary = document.createElement("summary");
  summary.className = "provider-model-group-summary";
  const heading = document.createElement("div");
  heading.className = "provider-model-group-heading";
  const name = document.createElement("h4");
  name.textContent = upstream.display_name;
  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("图像输入"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("工具调用"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("思考档位"));
  }
  heading.append(name, capabilities);
  const meta = document.createElement("span");
  meta.className = "provider-model-group-meta";
  meta.textContent = `${virtualModels.length} 个 IDE 入口`;
  summary.append(heading, meta);
  const variants = document.createElement("div");
  variants.className = "provider-model-variants";
  for (const virtualModel of virtualModels) {
    variants.append(providerModelRow(provider, virtualModel, upstream));
  }
  group.append(summary, variants);
  return group;
}

function providerModelRow(
  provider: Provider,
  virtualModel: VirtualModel,
  upstream: UpstreamModel,
): HTMLElement {
  const row = document.createElement("div");
  row.className = "provider-model-variant";
  row.dataset.virtualModelId = virtualModel.id;
  const content = document.createElement("div");
  content.className = "variant-main";
  const titleRow = document.createElement("div");
  titleRow.className = "model-title-row";
  const title = document.createElement("h5");
  title.textContent = configuredModelDisplayName(
    virtualModel.display_name,
    provider.name,
    virtualModel.default_reasoning_level,
    Object.keys(upstream.capabilities.reasoning.levels).length > 0,
  );
  titleRow.append(title);
  const meta = document.createElement("p");
  meta.className = "muted compact";
  meta.textContent = virtualModel.default_reasoning_level
    ? `思考档位：${reasoningLevelLabel(virtualModel.default_reasoning_level)}`
    : Object.keys(upstream.capabilities.reasoning.levels).length > 0
      ? "思考档位：模型默认"
      : "普通 IDE 模型入口";
  const technical = document.createElement("details");
  technical.className = "variant-technical";
  const technicalSummary = document.createElement("summary");
  technicalSummary.textContent = "技术详情";
  const technicalValue = document.createElement("code");
  technicalValue.textContent = `Host ID: ${effectiveHostModelId(virtualModel)} · VirtualModel: ${virtualModel.id}`;
  technical.append(technicalSummary, technicalValue);
  content.append(titleRow, meta, technical);

  const connectionResult = document.createElement("p");
  connectionResult.className = "connection-result";
  connectionResult.setAttribute("role", "status");
  connectionResult.setAttribute("aria-live", "polite");
  connectionResult.hidden = true;
  content.append(connectionResult);

  const actions = document.createElement("div");
  actions.className = "variant-actions";
  const testConnection = document.createElement("button");
  testConnection.type = "button";
  testConnection.className = "secondary compact-button model-test-button";
  testConnection.textContent = "测试 IDE 路由";
  testConnection.addEventListener("click", () => {
    void withBusy(testConnection, async () => {
      await testVirtualModelConnection(virtualModel.id, connectionResult);
      const providerVirtualIds = config.virtual_models.flatMap((model) => {
        const candidateUpstream = config.upstream_models.find(
          (item) => item.id === model.upstream_model_id && item.provider_id === provider.id,
        );
        return candidateUpstream ? [model.id] : [];
      }).sort();
      const existingSession = providerTestSessions.get(provider.id);
      if (
        existingSession
        && JSON.stringify([...existingSession.targetVirtualModelIds].sort())
          === JSON.stringify(providerVirtualIds)
      ) {
        existingSession.completedAt = Date.now();
      } else {
        providerTestSessions.delete(provider.id);
      }
      window.setTimeout(renderProviders, 0);
    }, "测试中…");
  });
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "danger-text";
  remove.textContent = "移除入口";
  armDestructiveButton(
    remove,
    "确认移除入口",
    () => removeModel(virtualModel.id, remove),
    () => destructiveMutationBlocker(new Set([virtualModel.id])),
  );
  actions.append(testConnection, remove);
  const existingState = connectionTestResults.get(virtualModel.id);
  if (existingState) renderConnectionTestState(connectionResult, existingState);
  row.append(content, actions);
  return row;
}

async function withConnectionTestSlot<T>(action: () => Promise<T>): Promise<T> {
  if (activeConnectionTests < 3) {
    activeConnectionTests += 1;
  } else {
    await new Promise<void>((resolve) => connectionTestWaiters.push(resolve));
  }

  try {
    return await action();
  } finally {
    const next = connectionTestWaiters.shift();
    if (next) next();
    else activeConnectionTests -= 1;
  }
}

function sharedConnectionTest(virtualModelId: string): Promise<ModelConnectionTestOutcome> {
  const existingTest = connectionTestsInFlight.get(virtualModelId);
  if (existingTest) return existingTest;

  const test = withConnectionTestSlot(async () => {
    try {
      const result = await invoke<ModelConnectionTestResult>("test_model_connection", {
        virtualModelId,
      });
      return { kind: "result", result } as const;
    } catch (error) {
      return { kind: "error", message: errorMessage(error) } as const;
    }
  });
  connectionTestsInFlight.set(virtualModelId, test);
  const clear = () => {
    if (connectionTestsInFlight.get(virtualModelId) === test) {
      connectionTestsInFlight.delete(virtualModelId);
    }
  };
  void test.then(clear, clear);
  return test;
}

function renderConnectionTestState(target: HTMLElement, state: ConnectionTestViewState): void {
  target.hidden = false;
  target.className = `connection-result ${state.status === "testing" ? "pending" : state.status}`;
  target.textContent = state.message;
  target.title = state.status === "error" ? state.message : "";
}

async function testVirtualModelConnection(
  virtualModelId: string,
  target: HTMLElement,
): Promise<boolean> {
  const pending: ConnectionTestViewState = { status: "testing", message: "测试中…" };
  connectionTestResults.set(virtualModelId, pending);
  renderConnectionTestState(target, pending);

  const outcome = await sharedConnectionTest(virtualModelId);
  if (outcome.kind === "result") {
    const state: ConnectionTestViewState = outcome.result.success
      ? {
          status: "success",
          message: `测试通过 · ${outcome.result.durationMs} ms`,
          durationMs: outcome.result.durationMs,
        }
      : { status: "error", message: `测试失败 · ${outcome.result.message}` };
    connectionTestResults.set(virtualModelId, state);
    renderConnectionTestState(target, state);
    return outcome.result.success;
  }

  const state: ConnectionTestViewState = {
    status: "error",
    message: `测试失败 · ${outcome.message}`,
  };
  connectionTestResults.set(virtualModelId, state);
  renderConnectionTestState(target, state);
  return false;
}

async function testProviderModels(
  providerId: string,
  card: HTMLElement,
  virtualModels: VirtualModel[],
  sessionVirtualModelIds: string[],
  progressButton: HTMLButtonElement,
): Promise<void> {
  const rows = [...card.querySelectorAll<HTMLElement>(".provider-model-variant")];
  const resultTargets = new Map(rows.map((row) => [
    row.dataset.virtualModelId,
    row.querySelector<HTMLElement>(".connection-result"),
  ]));
  const rowTestButtons = [...card.querySelectorAll<HTMLButtonElement>(".model-test-button")];
  for (const button of rowTestButtons) {
    button.dataset.bulkBusy = "true";
    button.disabled = true;
  }

  let nextIndex = 0;
  let completed = 0;
  let succeeded = 0;
  const worker = async () => {
    while (nextIndex < virtualModels.length) {
      const virtualModel = virtualModels[nextIndex];
      nextIndex += 1;
      const target = resultTargets.get(virtualModel.id);
      if (target && await testVirtualModelConnection(virtualModel.id, target)) {
        succeeded += 1;
      }
      completed += 1;
      progressButton.textContent = `测试 ${completed}/${virtualModels.length}`;
    }
  };

  try {
    const concurrency = Math.min(3, virtualModels.length);
    await Promise.all(Array.from({ length: concurrency }, worker));
  } finally {
    for (const button of rowTestButtons) {
      delete button.dataset.bulkBusy;
      button.disabled = button.dataset.busy === "true"
        || button.dataset.unavailable === "true";
    }
  }

  const failed = virtualModels.length - succeeded;
  providerTestSessions.set(providerId, {
    targetVirtualModelIds: sessionVirtualModelIds,
    completedAt: Date.now(),
  });
  showNotice(
    `测试完成：${succeeded} 个通过，${failed} 个失败`,
    failed > 0 ? "error" : "success",
  );
  window.setTimeout(renderProviders, 0);
}



function destructiveMutationBlocker(removedIds: Set<string>): string | null {
  if (providerEditorDirty) {
    return "当前有未保存的上游服务修改，请先保存或取消编辑";
  }
  return fallbackRemovalBlocker(removedIds);
}

function fallbackRemovalBlocker(removedIds: Set<string>): string | null {
  const source = config.virtual_models.find(
    (model) => !removedIds.has(model.id)
      && model.fallback_virtual_model_id
      && removedIds.has(model.fallback_virtual_model_id),
  );
  if (!source?.fallback_virtual_model_id) return null;
  const removed = config.virtual_models.find(
    (model) => model.id === source.fallback_virtual_model_id,
  );
  return `无法删除：IDE 入口“${source.display_name}”仍将“${removed?.display_name ?? source.fallback_virtual_model_id}”用作备用模型。请先调整 fallback。`;
}

function armDestructiveButton(
  button: HTMLButtonElement,
  confirmLabel: string,
  action: () => Promise<void>,
  beforeArm?: () => string | null,
): void {
  const initialLabel = button.textContent ?? "删除";
  let armed = false;
  let resetTimer: number | null = null;
  const reset = () => {
    armed = false;
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    resetTimer = null;
    button.textContent = initialLabel;
    button.classList.remove("danger-confirm");
  };
  button.addEventListener("click", () => {
    if (!armed) {
      const blocker = beforeArm?.();
      if (blocker) {
        showNotice(blocker, "error");
        return;
      }
      armed = true;
      button.textContent = confirmLabel;
      button.classList.add("danger-confirm");
      resetTimer = window.setTimeout(reset, 4000);
      return;
    }
    const blocker = beforeArm?.();
    if (blocker) {
      reset();
      showNotice(blocker, "error");
      return;
    }
    void action().finally(reset);
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

function virtualModelCatalogKey(model: VirtualModel): string {
  return model.id.startsWith("custom-") ? model.id : `custom-${model.id}`;
}

function findVirtualModelByAcceptedId(modelId: string): VirtualModel | undefined {
  return config.virtual_models.find((model) =>
    model.id === modelId
    || effectiveHostModelId(model) === modelId
    || virtualModelCatalogKey(model) === modelId
  );
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

function stripConfiguredModelSuffix(modelName: string, providerName: string): string {
  const knownSuffixes = [
    ` · ${providerName}`,
    ...["default", "off", "low", "medium", "high", "xhigh", "max", "auto"]
      .map((level) => ` ${level}(${providerName})`),
    `(${providerName})`,
  ];
  return knownSuffixes.reduce(
    (name, knownSuffix) => name.endsWith(knownSuffix) ? name.slice(0, -knownSuffix.length) : name,
    modelName,
  );
}

function configuredModelDisplayName(
  modelName: string,
  providerName: string,
  reasoningLevel: ReasoningLevel | null,
  supportsReasoning: boolean,
): string {
  const baseName = stripConfiguredModelSuffix(modelName, providerName);
  if (!supportsReasoning) return `${baseName}(${providerName})`;

  const variant = reasoningLevel ?? "default";
  return `${baseName} ${variant.replace("_", "")}(${providerName})`;
}

function reasoningLevelLabel(level: ReasoningLevel): string {
  return {
    off: "Off",
    low: "Low",
    medium: "Medium",
    high: "High",
    x_high: "Extra High",
    max: "Max",
    auto: "Auto",
  }[level];
}

function reasoningVariantLabel(variant: ReasoningVariant): string {
  return variant === "default" ? "Default" : reasoningLevelLabel(variant);
}

function configurableReasoningLevels(protocol: ProviderProtocol): ConfigurableReasoningLevel[] {
  return protocol === "gemini"
    ? ["low", "medium", "high"]
    : ["low", "medium", "high", "x_high", "max"];
}

function reasoningVariantsFor(
  protocol: ProviderProtocol,
  virtualModels: VirtualModel[],
): Set<ReasoningVariant> {
  const configurable = new Set<ReasoningLevel>(configurableReasoningLevels(protocol));
  const variants = new Set<ReasoningVariant>();
  for (const virtualModel of virtualModels) {
    const level = virtualModel.default_reasoning_level;
    variants.add(level && configurable.has(level) ? level as ConfigurableReasoningLevel : "default");
  }
  if (variants.size === 0) variants.add("default");
  return variants;
}

function reasoningLevels(protocol: ProviderProtocol): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  if (protocol === "anthropic") {
    return {
      low: { kind: "budget_tokens", value: 1024 },
      medium: { kind: "budget_tokens", value: 4096 },
      high: { kind: "budget_tokens", value: 8192 },
      x_high: { kind: "budget_tokens", value: 16384 },
      max: { kind: "budget_tokens", value: 32768 },
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
    x_high: { kind: "effort", value: "xhigh" },
    max: { kind: "effort", value: "max" },
  };
}

function pruneConnectionTestState(): void {
  const validVirtualIds = new Set(config.virtual_models.map((model) => model.id));
  for (const id of connectionTestResults.keys()) {
    if (!validVirtualIds.has(id)) connectionTestResults.delete(id);
  }
  const validProviderIds = new Set(config.providers.map((provider) => provider.id));
  for (const id of providerTestSessions.keys()) {
    if (!validProviderIds.has(id)) providerTestSessions.delete(id);
  }
}

async function persistConfig(next: AppConfig): Promise<void> {
  if (configMutationInProgress) {
    throw new Error("另一项配置变更正在处理，请稍后重试");
  }
  configMutationInProgress = true;
  try {
    config = await invoke<AppConfig>("save_config", { config: next });
    pruneConnectionTestState();
    renderProviders();
  } finally {
    configMutationInProgress = false;
  }
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
      proxy_port: config.proxy_port,
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
      proxy_port: config.proxy_port,
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
  globalCatalogReasoningVariants = new Set<ReasoningVariant>(["default"]);
  catalogReasoningVariantsByModel = new Map();
  catalogVisionEnabledModelIds = new Set();
  catalogToolsEnabledModelIds = new Set();
  catalogReasoningEnabledModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
  catalogModelList.replaceChildren();
  element<HTMLElement>("#global-reasoning-options").replaceChildren();
  catalogResults.hidden = true;
  element<HTMLInputElement>("#catalog-search").value = "";
  element<HTMLInputElement>("#select-all-models").checked = false;
  saveProviderButton.disabled = true;
}

function resetProviderEditor(): void {
  editingProviderId = null;
  draftProviderId = `provider-${crypto.randomUUID()}`;
  providerForm.reset();
  element<HTMLButtonElement>("#toggle-api-key").textContent = "显示";
  element<HTMLInputElement>("#api-key").type = "password";
  element<HTMLElement>("#provider-form-title").textContent = "添加上游服务";
  element<HTMLElement>("#provider-form-kicker").textContent = "ADD UPSTREAM";
  resetCatalogResults();
  providerEditorDirty = false;
  providerDirtyBadge.hidden = true;
  invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

function closeProviderEditor(force = false): boolean {
  if (!force && !confirmDiscardProviderChanges()) return false;
  resetProviderEditor();
  providerFormPanel.open = false;
  return true;
}

function openProviderEditor(providerId: string | null = null): void {
  if (providerFormPanel.open && editingProviderId === providerId) {
    providerFormPanel.scrollIntoView({ behavior: "smooth", block: "start" });
    return;
  }
  if (!confirmDiscardProviderChanges()) return;
  resetProviderEditor();
  editingProviderId = providerId;
  const provider = providerId
    ? config.providers.find((item) => item.id === providerId)
    : undefined;
  if (provider) {
    draftProviderId = provider.id;
    element<HTMLElement>("#provider-form-title").textContent = `管理上游服务 · ${provider.name}`;
    element<HTMLElement>("#provider-form-kicker").textContent = "EDIT UPSTREAM";
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
  invalidatePendingProviderSave();
  refreshProviderEditorControls();
  const provider = providerFromForm();
  const fetched = await invoke<ProviderCatalogModel[]>("fetch_provider_catalog", { provider });
  const fetchedIds = new Set(fetched.map((model) => model.id));
  const byId = new Map(fetched.map((model) => [model.id, model]));
  const existingUpstreams = editingProviderId
    ? config.upstream_models.filter((item) => item.provider_id === editingProviderId)
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
  globalCatalogReasoningVariants = new Set();
  for (const upstream of existingUpstreams) {
    if (!catalogReasoningEnabledModelIds.has(upstream.upstream_model_id)) continue;
    const virtualModels = config.virtual_models.filter(
      (item) => item.upstream_model_id === upstream.id,
    );
    for (const variant of reasoningVariantsFor(provider.protocol, virtualModels)) {
      globalCatalogReasoningVariants.add(variant);
    }
  }
  if (globalCatalogReasoningVariants.size === 0) {
    globalCatalogReasoningVariants.add("default");
  }
  catalogReasoningVariantsByModel = new Map(catalogModels.map((model) => {
    const upstream = existingUpstreamsByModelId.get(model.id);
    if (!upstream) return [model.id, new Set(globalCatalogReasoningVariants)];
    const virtualModels = config.virtual_models.filter(
      (item) => item.upstream_model_id === upstream.id,
    );
    return [model.id, reasoningVariantsFor(provider.protocol, virtualModels)];
  }));
  catalogResults.hidden = false;
  element<HTMLElement>("#catalog-status").textContent = legacyCatalogModelIds.size > 0
    ? `目录获取成功 · ${fetched.length} 个模型 · ${legacyCatalogModelIds.size} 个已配置模型未返回`
    : `目录获取成功 · ${fetched.length} 个模型`;
  renderGlobalReasoningOptions(provider.protocol);
  renderCatalogModels();
  catalogResults.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function renderGlobalReasoningOptions(protocol: ProviderProtocol): void {
  const container = element<HTMLElement>("#global-reasoning-options");
  container.replaceChildren();
  const availableVariants: ReasoningVariant[] = [
    "default",
    ...configurableReasoningLevels(protocol),
  ];
  for (const variant of availableVariants) {
    const option = document.createElement("label");
    option.className = "check-label";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = globalCatalogReasoningVariants.has(variant);
    checkbox.addEventListener("change", () => {
      if (!checkbox.checked && globalCatalogReasoningVariants.size === 1) {
        checkbox.checked = true;
        showNotice("思考档位模板至少保留一个选项", "error");
        return;
      }
      if (checkbox.checked) globalCatalogReasoningVariants.add(variant);
      else globalCatalogReasoningVariants.delete(variant);
    });
    const label = document.createElement("span");
    label.textContent = reasoningVariantLabel(variant);
    option.append(checkbox, label);
    container.append(option);
  }
  applyReasoningTemplateButton.disabled = catalogReasoningEnabledModelIds.size === 0;
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

function renderCatalogModels(): void {
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    const selected = selectedCatalogModelIds.has(model.id);
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
      legacy.title = "保留选择不会删除现有配置；取消后保存将移除对应 IDE 入口";
      copy.append(legacy);
    }
    copy.append(id);
    select.append(checkbox, copy);

    const capabilities = document.createElement("div");
    capabilities.className = "catalog-model-capabilities";
    capabilities.append(
      catalogCapabilityToggle(model.id, "图像输入", catalogVisionEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        setProviderEditorDirty(true);
      }),
      catalogCapabilityToggle(model.id, "工具调用", catalogToolsEnabledModelIds, () => {
        changedCatalogCapabilityModelIds.add(model.id);
        setProviderEditorDirty(true);
      }),
      catalogCapabilityToggle(model.id, "思考档位", catalogReasoningEnabledModelIds, () => {
        if (catalogReasoningEnabledModelIds.has(model.id)) {
          catalogReasoningVariantsByModel.set(model.id, new Set(globalCatalogReasoningVariants));
        }
        changedCatalogReasoningModelIds.add(model.id);
        applyReasoningTemplateButton.disabled = catalogReasoningEnabledModelIds.size === 0;
        setProviderEditorDirty(true);
      }),
    );
    for (const input of capabilities.querySelectorAll<HTMLInputElement>("input")) {
      input.disabled = !selected;
    }

    const test = document.createElement("button");
    test.type = "button";
    test.className = "secondary compact-button";
    test.textContent = "测试上游生成";
    const result = document.createElement("span");
    result.className = "catalog-model-test-result";
    result.setAttribute("role", "status");
    test.addEventListener("click", () => {
      void withProviderEditorBusy(test, async () => {
        result.className = "catalog-model-test-result pending";
        result.textContent = "测试中…";
        const response = await invoke<ModelConnectionTestResult>(
          "test_provider_model_connection",
          { provider: providerFromForm(), upstreamModelId: model.id },
        );
        result.className = `catalog-model-test-result ${response.success ? "success" : "error"}`;
        result.textContent = response.success
          ? `测试通过 · ${response.durationMs} ms`
          : `测试失败 · ${response.message}`;
        result.title = response.message;
      }, "测试中…");
    });
    const actions = document.createElement("div");
    actions.className = "catalog-model-actions";
    actions.append(capabilities, result, test);
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

function summarizeProviderChanges(
  providerId: string,
  nextConfig: AppConfig,
): ProviderChangeSummary {
  const currentUpstreams = config.upstream_models.filter((item) => item.provider_id === providerId);
  const nextUpstreams = nextConfig.upstream_models.filter((item) => item.provider_id === providerId);
  const currentUpstreamIds = new Set(currentUpstreams.map((item) => item.id));
  const nextUpstreamIds = new Set(nextUpstreams.map((item) => item.id));
  const currentVirtuals = config.virtual_models.filter(
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
  providerChangeSummary.replaceChildren();
  providerChangeSummary.hidden = false;
  providerChangeSummary.className = `provider-change-summary${summary.fallbackBlockers.length > 0 ? " blocked" : summary.removedVirtualModels.length > 0 ? " destructive" : ""}`;
  const title = document.createElement("strong");
  title.textContent = summary.fallbackBlockers.length > 0 ? "当前变更无法保存" : "保存影响";
  const list = document.createElement("ul");
  const lines = [
    `上游模型：新增 ${summary.addedUpstreamIds.length}，移除 ${summary.removedUpstreamIds.length}`,
    `IDE 入口：新增 ${summary.addedVirtualModels.length}，保留 ${summary.retainedVirtualCount}，移除 ${summary.removedVirtualModels.length}`,
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
    removedSummary.textContent = "查看将移除的 IDE 入口";
    const names = document.createElement("p");
    names.textContent = summary.removedVirtualModels.map((model) => model.display_name).join("、");
    removed.append(removedSummary, names);
    providerChangeSummary.append(title, list, removed);
  } else {
    providerChangeSummary.append(title, list);
  }
}

async function executeProviderSave(plan: ProviderSavePlan): Promise<void> {
  const currentUpstreamIds = new Set(
    config.upstream_models
      .filter((upstream) => upstream.provider_id === plan.provider.id)
      .map((upstream) => upstream.id),
  );
  for (const virtualModel of config.virtual_models) {
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
  closeProviderEditor(true);
  showNotice(`${plan.wasEditing ? "已更新" : "已添加"}上游服务 ${plan.provider.name}：当前 ${currentCount} 个 IDE 入口`);
}

async function saveProvider(): Promise<void> {
  if (pendingProviderSavePlan) {
    const plan = pendingProviderSavePlan;
    pendingProviderSavePlan = null;
    await executeProviderSave(plan);
    return;
  }
  if (!providerForm.reportValidity() || selectedCatalogModelIds.size === 0) return;
  const provider = providerFromForm();
  const previousProvider = editingProviderId
    ? config.providers.find((item) => item.id === editingProviderId)
    : undefined;
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
  const protocolChanged = previousProvider !== undefined
    && previousProvider.protocol !== provider.protocol;
  const nextUpstreams: UpstreamModel[] = [];
  const nextVirtuals: VirtualModel[] = [];
  const reasoningVariantsForModel = (modelId: string): Set<ReasoningVariant> =>
    catalogReasoningEnabledModelIds.has(modelId)
      ? catalogReasoningVariantsByModel.get(modelId) ?? new Set(globalCatalogReasoningVariants)
      : new Set<ReasoningVariant>(["default"]);

  for (const model of selectedModels) {
    const existingUpstream = providerUpstreams.find(
      (item) => item.upstream_model_id === model.id,
    );
    if (!existingUpstream) continue;

    const existingVirtuals = config.virtual_models.filter(
      (item) => item.upstream_model_id === existingUpstream.id,
    );
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    if (!reasoningChanged) {
      for (const virtualModel of existingVirtuals) {
        occupiedHostModelIds.add(effectiveHostModelId(virtualModel));
      }
      continue;
    }

    const reasoningVariants = reasoningVariantsForModel(model.id);
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      [...reasoningVariants].map((variant) => variant === "default" ? null : variant),
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
      ? config.virtual_models.filter((item) => item.upstream_model_id === existingUpstream.id)
      : [];
    const reasoningChanged = changedCatalogReasoningModelIds.has(model.id) || protocolChanged;
    const capabilitiesChanged = changedCatalogCapabilityModelIds.has(model.id);
    const vision = catalogVisionEnabledModelIds.has(model.id);
    const tools = catalogToolsEnabledModelIds.has(model.id);
    const id = crypto.randomUUID();
    const upstreamId = existingUpstream?.id ?? `upstream-${id}`;

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
    const reasoningVariants = reasoningVariantsForModel(model.id);
    const availableMappings = reasoningLevels(protocol);
    const enabledLevels = reasoningEnabled
      ? reasoningVariants.has("default")
        ? configurableReasoningLevels(protocol)
        : [...reasoningVariants].filter(
            (variant): variant is ConfigurableReasoningLevel => variant !== "default",
          )
      : [];
    const levels: Partial<Record<ReasoningLevel, ReasoningMapping>> = reasoningEnabled
        && reasoningVariants.has("default")
        && !protocolChanged
      ? { ...existingUpstream?.capabilities.reasoning.levels }
      : {};
    for (const level of enabledLevels) {
      const mapping = (protocolChanged
        ? undefined
        : existingUpstream?.capabilities.reasoning.levels[level])
        ?? availableMappings[level];
      if (mapping) levels[level] = mapping;
    }
    const reasoning = { levels };
    const retainedReasoningLevels = new Set<ReasoningLevel | null>(
      [...reasoningVariants].map((variant) => variant === "default" ? null : variant),
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

    for (const variant of reasoningVariants) {
      const defaultReasoningLevel = variant === "default" ? null : variant;
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
    ? config.providers.map((item) => item.id === provider.id ? provider : item)
    : [...config.providers, provider];
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
    proxy_port: config.proxy_port,
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
    showNotice("请确认保存并移除列出的 IDE 入口", "error");
    return;
  }
  await executeProviderSave(plan);
}

function activityMetric(label: string, value: string): HTMLDivElement {
  const metric = document.createElement("div");
  const term = document.createElement("dt");
  term.textContent = label;
  const detail = document.createElement("dd");
  detail.textContent = value;
  metric.append(term, detail);
  return metric;
}

function formatActivityTime(timestampMs: number): { label: string; dateTime: string | null } {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return { label: "时间未知", dateTime: null };
  return {
    label: new Intl.DateTimeFormat("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(date),
    dateTime: date.toISOString(),
  };
}

function formatDuration(durationMs: number): string {
  return durationMs >= 1000 ? `${(durationMs / 1000).toFixed(2)} s` : `${durationMs} ms`;
}

function isActivityFailure(item: ActivityItem): boolean {
  return item.statusCode < 200 || item.statusCode >= 300 || item.errorCategory !== null;
}

function resolveActivityContext(item: ActivityItem): {
  requestedName: string;
  actualRouteName: string;
  upstreamName: string;
  providerName: string;
} {
  const resolveVirtualModelName = (virtualModelId: string): string => {
    const virtualModel = findVirtualModelByAcceptedId(virtualModelId);
    const upstream = virtualModel
      ? config.upstream_models.find((model) => model.id === virtualModel.upstream_model_id)
      : undefined;
    const provider = upstream
      ? config.providers.find((candidate) => candidate.id === upstream.provider_id)
      : undefined;
    return virtualModel && upstream && provider
      ? configuredModelDisplayName(
          virtualModel.display_name,
          provider.name,
          virtualModel.default_reasoning_level,
          Object.keys(upstream.capabilities.reasoning.levels).length > 0,
        )
      : virtualModelId;
  };
  const requestedVirtualModelId = item.requestedVirtualModelId ?? item.virtualModelId;
  const actualVirtualModel = findVirtualModelByAcceptedId(item.virtualModelId);
  const actualUpstream = actualVirtualModel
    ? config.upstream_models.find((model) => model.id === actualVirtualModel.upstream_model_id)
    : undefined;
  const actualProvider = config.providers.find(
    (candidate) => candidate.id === (actualUpstream?.provider_id ?? item.providerId),
  );
  return {
    requestedName: resolveVirtualModelName(requestedVirtualModelId),
    actualRouteName: resolveVirtualModelName(item.virtualModelId),
    upstreamName: actualUpstream?.upstream_model_id ?? item.upstreamModelId ?? "—",
    providerName: actualProvider?.name ?? item.providerId,
  };
}

function formatTokenUsage(item: ActivityItem): string {
  if (item.promptTokens === null && item.completionTokens === null) return "—";
  return `输入 ${item.promptTokens ?? "—"} · 输出 ${item.completionTokens ?? "—"}`;
}

function renderActivityLog(): void {
  const failures = activityItems.filter(isActivityFailure).length;
  const visibleItems = activityFailedOnly
    ? activityItems.filter(isActivityFailure)
    : activityItems;
  activityCount.textContent = activityFailedOnly
    ? `失败 ${visibleItems.length} / 共 ${activityItems.length} 条`
    : `最近 ${activityItems.length} 条 · 失败 ${failures}`;
  activityCount.setAttribute("aria-label", activityCount.textContent);
  setButtonUnavailable(clearActivityButton, activityItems.length === 0);
  const oldScrollTop = activityList.scrollTop;
  const oldScrollHeight = activityList.scrollHeight;
  const nearTop = oldScrollTop < 24;
  activityList.replaceChildren();

  if (visibleItems.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = activityItems.length === 0
      ? "暂无调用日志。通过本地代理发起模型请求后，记录会显示在这里。"
      : "当前没有失败日志。";
    activityList.append(empty);
    return;
  }

  for (const item of visibleItems) {
    const failed = isActivityFailure(item);
    const context = resolveActivityContext(item);
    const card = document.createElement("article");
    card.className = `activity-item ${failed ? "error" : "success"}`;
    const heading = document.createElement("div");
    heading.className = "activity-item-heading";
    const timestamp = document.createElement("time");
    const formattedTime = formatActivityTime(item.timestampMs);
    timestamp.textContent = formattedTime.label;
    if (formattedTime.dateTime) timestamp.dateTime = formattedTime.dateTime;
    const status = document.createElement("span");
    status.className = `status-pill ${failed ? "error" : "success"}`;
    status.textContent = failed
      ? item.fallbackAttempted ? "失败 · Fallback 未成功" : "失败"
      : item.fallbackSucceeded ? "成功 · Fallback" : "成功";
    heading.append(timestamp, status);

    const route = document.createElement("div");
    route.className = "activity-route";
    for (const [label, value, title] of [
      ["IDE 模型入口", context.requestedName, item.requestedVirtualModelId ?? item.virtualModelId],
      ["实际路由", context.actualRouteName, item.virtualModelId],
      ["实际上游", context.upstreamName, item.upstreamModelId ?? ""],
      ["上游服务 / 协议", `${context.providerName} / ${item.providerProtocol ?? "未知"}`, item.providerId],
    ]) {
      const entry = document.createElement("div");
      const entryLabel = document.createElement("span");
      entryLabel.textContent = label;
      const entryValue = document.createElement("code");
      entryValue.textContent = value;
      entryValue.title = title;
      entry.append(entryLabel, entryValue);
      route.append(entry);
    }

    const metrics = document.createElement("dl");
    metrics.className = "activity-metrics";
    metrics.append(
      activityMetric("请求", item.stream ? "流式" : "非流式"),
      activityMetric("消息", String(item.messageCount)),
      activityMetric("工具", String(item.toolCount)),
      activityMetric("耗时", formatDuration(item.durationMs)),
      activityMetric("HTTP", item.statusCode > 0 ? String(item.statusCode) : "无响应"),
      activityMetric(
        "路由",
        item.fallbackAttempted
          ? item.fallbackSucceeded ? "Fallback 成功" : "Fallback 尝试失败"
          : "主模型",
      ),
    );
    if (item.promptTokens !== null || item.completionTokens !== null) {
      metrics.append(activityMetric("Token", formatTokenUsage(item)));
    }

    card.append(heading, route, metrics);
    if (failed) {
      const error = document.createElement("div");
      error.className = "activity-error";
      const errorHeading = document.createElement("div");
      errorHeading.className = "activity-error-heading";
      const category = document.createElement("strong");
      category.textContent = item.errorCategory ?? "未分类错误";
      const copy = document.createElement("button");
      copy.type = "button";
      copy.className = "quiet activity-copy-error";
      copy.textContent = "复制错误";
      copy.addEventListener("click", () => {
        const text = [
          `时间: ${formattedTime.label}`,
          `IDE 模型入口: ${context.requestedName}`,
          `实际路由: ${context.actualRouteName}`,
          `实际上游: ${context.upstreamName}`,
          `上游服务: ${context.providerName}`,
          `HTTP: ${item.statusCode || "无响应"}`,
          `错误分类: ${item.errorCategory ?? "未分类错误"}`,
          `错误详情: ${item.errorDetail ?? "未提供错误详情"}`,
        ].join("\n");
        void navigator.clipboard.writeText(text)
          .then(() => showNotice("错误信息已复制"))
          .catch((copyError) => showNotice(`复制失败：${errorMessage(copyError)}`, "error"));
      });
      errorHeading.append(category, copy);
      const detail = document.createElement("p");
      detail.textContent = item.errorDetail ?? "未提供错误详情";
      error.append(errorHeading, detail);
      card.append(error);
    }
    activityList.append(card);
  }
  if (!nearTop) {
    activityList.scrollTop = oldScrollTop + (activityList.scrollHeight - oldScrollHeight);
  }
}

function setActivityItems(items: ActivityItem[]): void {
  activityItems = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
  activitySnapshot = JSON.stringify(activityItems);
  renderActivityLog();
}

async function refreshActivityLog(silent = false): Promise<void> {
  if (activityRefreshInFlight) return activityRefreshInFlight;
  const requestVersion = activityRequestVersion;
  const task = (async () => {
    try {
      const items = await invoke<ActivityItem[]>("get_activity_log");
      if (requestVersion !== activityRequestVersion) return;
      const ordered = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
      const snapshot = JSON.stringify(ordered);
      if (snapshot !== activitySnapshot) setActivityItems(ordered);
    } catch (error) {
      if (!silent) throw error;
    }
  })();
  activityRefreshInFlight = task;
  try {
    await task;
  } finally {
    if (activityRefreshInFlight === task) activityRefreshInFlight = null;
  }
}

async function clearActivityLog(): Promise<void> {
  activityActionInProgress = true;
  activityRequestVersion += 1;
  try {
    await invoke<void>("clear_activity_log");
    activityRequestVersion += 1;
    setActivityItems([]);
    showNotice("内存调用日志已清空");
  } finally {
    activityActionInProgress = false;
  }
}



async function refreshIde(): Promise<void> {
  renderIde(await invoke<IdeStatus>("discover_ide"));
}

async function initialize(): Promise<void> {
  const [configResult, proxyResult, ideResult, activityResult] = await Promise.allSettled([
    invoke<AppConfig>("get_config"),
    invoke<ProxyStatus>("proxy_status"),
    invoke<IdeStatus>("discover_ide"),
    invoke<ActivityItem[]>("get_activity_log"),
  ]);
  const failures: string[] = [];
  proxyStatusLoadFailed = proxyResult.status === "rejected";
  ideStatusLoadFailed = ideResult.status === "rejected";
  if (configResult.status === "fulfilled") {
    config = configResult.value;
    renderProviders();
  } else {
    failures.push("上游服务配置");
    providerList.replaceChildren();
    const error = document.createElement("p");
    error.className = "empty-state error-state";
    error.textContent = `配置读取失败：${errorMessage(configResult.reason)}`;
    providerList.append(error);
  }
  if (proxyResult.status === "fulfilled") renderProxy(proxyResult.value);
  else {
    failures.push("代理状态");
    element<HTMLElement>("#proxy-state").textContent = "读取失败";
    setReadinessStep("#readiness-proxy", "#readiness-proxy-value", "attention", "读取失败");
  }
  if (ideResult.status === "fulfilled") renderIde(ideResult.value);
  else {
    failures.push("IDE 状态");
    element<HTMLElement>("#ide-state").textContent = "读取失败";
    element<HTMLElement>("#ide-integration-state").textContent = "读取失败";
    setReadinessStep("#readiness-ide", "#readiness-ide-value", "attention", "读取失败");
  }
  if (activityResult.status === "fulfilled") setActivityItems(activityResult.value);
  else {
    failures.push("调用日志");
    activityList.replaceChildren();
    const error = document.createElement("p");
    error.className = "empty-state error-state";
    error.textContent = `调用日志读取失败：${errorMessage(activityResult.reason)}；可点击刷新重试。`;
    activityList.append(error);
  }
  if (failures.length > 0) {
    showNotice(`部分状态读取失败：${failures.join("、")}`, "error");
  }
}

startProxyButton.addEventListener("click", () => void withBusy(startProxyButton, async () => {
  const preferredPort = config.proxy_port;
  const status = await invoke<ProxyStatus>("start_proxy");
  renderProxy(status);
  await refreshIde();
  const actualPort = proxyPortFromAddress(status.address);
  showNotice(actualPort !== null && actualPort !== preferredPort
    ? `首选端口 ${preferredPort} 已占用，代理已切换到 ${actualPort}；请停用后重新启用 IDE 接入以更新地址`
    : "本地代理已启动");
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

refreshActivityButton.addEventListener("click", () => {
  void withBusy(refreshActivityButton, () => refreshActivityLog());
});

armDestructiveButton(
  clearActivityButton,
  "确认清空内存日志",
  () => withBusy(clearActivityButton, clearActivityLog),
);

openProviderFormButton.addEventListener("click", () => openProviderEditor());

element<HTMLButtonElement>("#cancel-provider").addEventListener("click", () => {
  closeProviderEditor();
});

enableIdeIntegrationButton.addEventListener("click", () => {
  void withBusy(enableIdeIntegrationButton, async () => {
    showNotice("正在启用 IDE 原生配置接入；运行中的 IDE 将自动重启…");
    const status = await invoke<IdeStatus>("enable_ide_integration");
    renderIde(status);
    showNotice(status.ideRunning
      ? "IDE 原生配置接入已启用，Antigravity IDE 已重启"
      : "IDE 原生配置接入已启用；请启动代理后打开 Antigravity IDE");
  });
});

launchIdeButton.addEventListener("click", () => void withBusy(launchIdeButton, async () => {
  await invoke<void>("launch_ide");
  showNotice("已启动厂商原版 Antigravity IDE");
}));

disableIdeIntegrationButton.addEventListener("click", () => {
  void withBusy(disableIdeIntegrationButton, async () => {
    showNotice("正在停用 IDE 原生配置接入；运行中的 IDE 将自动重启…");
    const status = await invoke<IdeStatus>("disable_ide_integration");
    renderIde(status);
    showNotice(status.ideRunning
      ? "IDE 原生配置接入已停用，Antigravity IDE 已重启"
      : "IDE 原生配置接入已停用，原 settings 已恢复");
  });
});

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

applyReasoningTemplateButton.addEventListener("click", () => {
  for (const modelId of catalogReasoningEnabledModelIds) {
    catalogReasoningVariantsByModel.set(modelId, new Set(globalCatalogReasoningVariants));
    changedCatalogReasoningModelIds.add(modelId);
  }
  setProviderEditorDirty(true);
  showNotice(`已将模板应用到 ${catalogReasoningEnabledModelIds.size} 个上游模型`);
});

element<HTMLElement>("#provider-form-summary").addEventListener("click", (event) => {
  if (providerFormPanel.open && !confirmDiscardProviderChanges()) event.preventDefault();
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
});

failedActivityOnlyCheckbox.addEventListener("change", () => {
  activityFailedOnly = failedActivityOnlyCheckbox.checked;
  renderActivityLog();
});

window.setInterval(() => {
  if (document.visibilityState === "visible" && !activityActionInProgress) {
    void refreshActivityLog(true);
  }
}, 2000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void refreshActivityLog(true);
});

void initialize();
