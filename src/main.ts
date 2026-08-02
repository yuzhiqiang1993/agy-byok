import { confirm } from "@tauri-apps/plugin-dialog";

import { invoke } from "@tauri-apps/api/core";

type ProviderProtocol =
  | "openai_chat_completions"
  | "openai_responses"
  | "anthropic_messages"
  | "gemini_generate_content";

type ReasoningLevel = "off" | "low" | "medium" | "high" | "x_high" | "max" | "auto";
type ConfigurableReasoningLevel = "low" | "medium" | "high" | "x_high" | "max";

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

interface ProviderCatalogReasoning {
  supported?: boolean;
  levels?: ReasoningLevel[];
}

interface ProviderCatalogModel {
  id: string;
  displayName: string;
  reasoning?: ProviderCatalogReasoning;
}

type ClientIntegrationState = "official" | "managed" | "external" | "mismatch" | "conflict" | "unavailable";
type ClientConfigurationState = "not_enabled" | "matched" | "not_running" | "service_stopped" | "needs_update" | "checking" | "unavailable";

interface IdeStatus {
  installed: boolean;
  compatible: boolean;
  ideRunning: boolean;
  proxyRunning: boolean;

  state: "not_installed" | "vendor_original" | "patched" | "modified" | "incompatible";
  appPath: string;
  appVersion: string | null;
  extensionVersion: string | null;
  extensionSha256: string | null;
  message: string;
  integrationState: ClientIntegrationState;
  settingsPath: string;
  integrationMessage: string;
  configurationState: ClientConfigurationState;
  configurationMessage: string;
  canEnableIntegration: boolean;
  canLaunchIde: boolean;
  canDisableIntegration: boolean;
}

interface AppStatus {
  installed: boolean;
  appRunning: boolean;
  proxyRunning: boolean;
  appPath: string;
  appVersion: string | null;
  lsPath: string;
  integrationState: ClientIntegrationState;
  integrationMessage: string;
  configurationState: ClientConfigurationState;
  configurationMessage: string;
  configuredEndpoint: string | null;
  canEnableIntegration: boolean;
  canLaunchApp: boolean;
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
let latestAppStatus: AppStatus | null = null;
let proxyStatusLoadFailed = false;
let ideStatusLoadFailed = false;
let appStatusLoadFailed = false;
let noticeTimer: number | null = null;
let editingProviderId: string | null = null;
let draftProviderId = `provider-${crypto.randomUUID()}`;
let catalogModels: ProviderCatalogModel[] = [];
let selectedCatalogModelIds = new Set<string>();
let catalogReasoningLevelsByModel = new Map<string, Set<ConfigurableReasoningLevel>>();
let catalogCustomReasoningByModel = new Map<string, string>();
let catalogVisionEnabledModelIds = new Set<string>();
let catalogToolsEnabledModelIds = new Set<string>();
let catalogReasoningEnabledModelIds = new Set<string>();
let activeReasoningModel: ProviderCatalogModel | null = null;
let activeProviderTabId: string | null = null;
let draftReasoningLevels = new Set<ConfigurableReasoningLevel>();
let changedCatalogCapabilityModelIds = new Set<string>();
let changedCatalogReasoningModelIds = new Set<string>();
let legacyCatalogModelIds = new Set<string>();
let providerEditorDirty = false;
let providerEditorBusy = false;
let providerEditorReturnFocus: HTMLElement | null = null;
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
const providerFormPanel = element<HTMLElement>("#provider-form-panel");
const openProviderFormButton = element<HTMLButtonElement>("#open-provider-form");
const catalogResults = element<HTMLElement>("#catalog-results");
const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
const cancelProviderButton = element<HTMLButtonElement>("#cancel-provider");
const saveProviderButton = element<HTMLButtonElement>("#save-provider");
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
const failedActivityOnlyCheckbox = element<HTMLInputElement>("#activity-failed-only");

const reasoningModal = element<HTMLDivElement>("#reasoning-modal");
const reasoningModalBackdrop = element<HTMLDivElement>("#reasoning-modal-backdrop");
const closeReasoningModalButton = element<HTMLButtonElement>("#close-reasoning-modal");
const cancelReasoningModalButton = element<HTMLButtonElement>("#cancel-reasoning-modal");
const confirmReasoningModalButton = element<HTMLButtonElement>("#confirm-reasoning-modal");
const reasoningModalTitle = element<HTMLElement>("#reasoning-modal-title");
const reasoningModalLevelsContainer = element<HTMLDivElement>("#reasoning-modal-levels");

providerFormPanel.hidden = true;
reasoningModal.hidden = true;

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

async function confirmHostAction(message: string, title: string): Promise<boolean> {
  try {
    return await confirm(message, { title, kind: "warning" });
  } catch (error) {
    console.error("Native confirm failed:", error);
    return window.confirm(message);
  }
}

function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
}

function integrationStateLabel(state: ClientIntegrationState): string {
  return {
    official: "未启用",
    managed: "已启用",
    external: "已启用",
    mismatch: "需要更新",
    conflict: "无法修改",
    unavailable: "未找到应用",
  }[state];
}

function integrationStateClass(state: ClientIntegrationState): string {
  if (state === "managed" || state === "external") return "success";
  if (state === "mismatch") return "warning";
  if (state === "conflict") return "error";
  return "neutral";
}

function displayIntegrationState(
  integrationState: ClientIntegrationState,
  configurationState: ClientConfigurationState,
): ClientIntegrationState {
  return configurationState === "needs_update" ? "mismatch" : integrationState;
}

function clientStatusMessage(
  integrationState: ClientIntegrationState,
  configurationState: ClientConfigurationState,
  configurationMessage: string,
): string {
  if (configurationMessage) return configurationMessage;
  if (configurationState === "needs_update") return "配置需要更新，请重新启用模型";
  if (configurationState === "service_stopped") return "模型已启用，请先启动本地服务";
  if (configurationState === "not_running") return "配置正常，启动应用后生效";
  if (configurationState === "checking") return "正在检查配置…";
  if (configurationState === "not_enabled") return "当前未启用模型";
  if (configurationState === "matched") return "配置正常";
  if (integrationState === "conflict") return "暂时无法修改，请关闭应用后刷新再试";
  return "未找到应用";
}

function clientConfigurationReady(state: ClientConfigurationState): boolean {
  return state === "matched" || state === "not_running";
}

function clientErrorMessage(error: unknown): string {
  const message = errorMessage(error);
  if (message.includes("请先启动") || message.includes("本地代理")) {
    return "请先启动本地服务。";
  }
  if (/App 接入|IDE settings|invalid application bundle|language_server|Wrapper|settings\.json/i.test(message)) {
    return "暂时无法修改，请关闭应用后刷新再试。";
  }
  return message;
}

function clientReady(state: ClientIntegrationState): boolean {
  return state === "managed" || state === "external";
}

function clientActionButtons(client: "ide" | "app"): HTMLButtonElement[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(`#${client}-actions button`));
}

async function withClientBusy<T>(
  button: HTMLButtonElement,
  client: "ide" | "app",
  action: () => Promise<T>,
  busyLabel = "处理中…",
): Promise<T | undefined> {
  if (button.dataset.busy === "true") return undefined;
  const buttons = clientActionButtons(client);
  if (buttons.some((item) => item.dataset.busy === "true")) return undefined;
  const labels = new Map(buttons.map((item) => [item, item.textContent ?? ""]));
  buttons.forEach((item) => {
    item.dataset.busy = "true";
    item.disabled = true;
  });
  button.textContent = busyLabel;
  let result: T | undefined;
  try {
    result = await action();
  } catch (error) {
    showNotice(clientErrorMessage(error), "error");
  } finally {
    buttons.forEach((item) => {
      item.dataset.busy = "false";
      item.textContent = labels.get(item) ?? item.textContent;
      item.disabled = item.dataset.unavailable === "true";
    });
  }
  return result;
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
      && clientReady(latestIdeStatus.integrationState)
      && clientConfigurationReady(latestIdeStatus.configurationState)
    : false;
  const appReady = latestAppStatus
    ? latestAppStatus.installed
      && latestAppStatus.integrationState === "managed"
      && clientConfigurationReady(latestAppStatus.configurationState)
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
        : !latestIdeStatus.installed
          ? "未安装"
          : integrationStateLabel(displayIntegrationState(
              latestIdeStatus.integrationState,
              latestIdeStatus.configurationState,
            )),
  );
  setReadinessStep(
    "#readiness-app",
    "#readiness-app-value",
    appStatusLoadFailed ? "attention" : latestAppStatus === null ? "pending" : appReady ? "ready" : "attention",
    appStatusLoadFailed
      ? "读取失败"
      : latestAppStatus === null
        ? "检查中"
        : !latestAppStatus.installed
          ? "未安装"
          : integrationStateLabel(displayIntegrationState(
              latestAppStatus.integrationState,
              latestAppStatus.configurationState,
            )),
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  readinessActionButton.hidden = false;
  readinessActionButton.onclick = null;
  if (modelCountValue === 0) {
    title.textContent = "先添加要使用的模型";
    detail.textContent = "添加模型后，就可以在 IDE 或 App 中启用。";
    readinessActionButton.textContent = "添加模型";
    readinessActionButton.onclick = () => {
      switchTab("tab-models");
      openProviderEditor();
    };
  } else if (proxyStatusLoadFailed || ideStatusLoadFailed || appStatusLoadFailed) {
    title.textContent = "部分运行状态读取失败";
    detail.textContent = "请使用对应客户端卡片的刷新操作重试。";
    readinessActionButton.hidden = true;
  } else if (latestProxyStatus === null || latestIdeStatus === null || latestAppStatus === null) {
    title.textContent = "正在准备…";
    detail.textContent = `已设置 ${modelCountValue} 个模型。`;
    readinessActionButton.hidden = true;
  } else if (!proxyRunning) {
    title.textContent = "模型已准备好，请启动服务";
    detail.textContent = "启动服务后，已启用的 IDE 或 App 才能使用这些模型。";
    readinessActionButton.textContent = "启动服务";
    readinessActionButton.onclick = () => void withBusy(readinessActionButton, startProxy);
  } else if (!ideReady && !appReady) {
    const canEnableIde = latestIdeStatus.canEnableIntegration;
    const canEnableApp = latestAppStatus.canEnableIntegration;
    if (canEnableIde) {
      title.textContent = "选择一个应用启用模型";
      detail.textContent = latestIdeStatus.ideRunning
        ? "启用后会自动重启正在运行的 IDE。"
        : "启用后，IDE 就可以使用自定义模型。";
      readinessActionButton.textContent = latestIdeStatus.ideRunning ? "启用并重启 IDE" : "启用 IDE";
      readinessActionButton.onclick = () => enableIdeIntegrationButton.click();
    } else if (canEnableApp) {
      title.textContent = "选择一个应用启用模型";
      detail.textContent = latestAppStatus.appRunning
        ? "启用后会自动重启正在运行的 App。"
        : "启用后，App 就可以使用自定义模型。";
      readinessActionButton.textContent = latestAppStatus.appRunning ? "启用并重启 App" : "启用 App";
      readinessActionButton.onclick = () => enableAppButton.click();
    } else {
      title.textContent = "暂时无法启用模型";
      detail.textContent = "请确认 IDE 或 App 已安装，然后刷新状态。";
      readinessActionButton.hidden = true;
    }
  } else if ((ideReady && latestIdeStatus.ideRunning) || (appReady && latestAppStatus.appRunning)) {
    const runningClients = [
      ideReady && latestIdeStatus.ideRunning ? "IDE" : null,
      appReady && latestAppStatus.appRunning ? "App" : null,
    ].filter((item): item is string => item !== null);
    title.textContent = `${runningClients.join("、")} 已启用模型`;
    detail.textContent = "现在可以直接使用自定义模型。";
    readinessActionButton.hidden = true;
  } else {
    title.textContent = "模型已启用";
    detail.textContent = "应用当前未运行，可以从下方启动。";
    readinessActionButton.hidden = true;
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
  cancelProviderButton.disabled = providerEditorBusy;
  if (!providerEditorBusy) {
    saveProviderButton.textContent = pendingProviderSavePlan
      ? `确认保存并移除 ${pendingProviderSavePlan.summary.removedVirtualModels.length} 个模型入口`
      : "保存上游服务";
  }
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

async function confirmDiscardProviderChanges(): Promise<boolean> {
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

  // 1. 代理状态微光脉冲呼吸灯
  const glowDot = document.querySelector("#proxy-glow-dot");
  if (glowDot) {
    if (running) {
      glowDot.classList.add("running");
    } else {
      glowDot.classList.remove("running");
    }
  }

  stopProxyButton.hidden = !running;
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

  state.textContent = status.ideRunning ? "运行中" : status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.ideRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = !status.installed
    ? "未找到 Antigravity IDE"
    : !status.compatible
      ? "当前版本暂时无法使用"
      : status.ideRunning
        ? "Antigravity IDE 正在运行"
        : "Antigravity IDE 已安装，当前未运行";

  const integrationState = element<HTMLSpanElement>("#ide-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#ide-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
  );

  enableIdeIntegrationButton.hidden = !status.canEnableIntegration;
  launchIdeButton.hidden = !status.canLaunchIde || status.ideRunning;
  disableIdeIntegrationButton.hidden = !status.canDisableIntegration;
  enableIdeIntegrationButton.textContent = status.ideRunning ? "启用并重启" : "启用模型";
  launchIdeButton.textContent = "启动 IDE";
  disableIdeIntegrationButton.textContent = status.ideRunning ? "停用并重启" : "停用模型";
  setButtonUnavailable(enableIdeIntegrationButton, !status.canEnableIntegration);
  setButtonUnavailable(launchIdeButton, !status.canLaunchIde);
  setButtonUnavailable(disableIdeIntegrationButton, !status.canDisableIntegration);
  renderReadiness();
}

function protocolName(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: "OpenAI · Chat Completions",
    openai_responses: "OpenAI · Responses API",
    anthropic_messages: "Anthropic · Messages API",
    gemini_generate_content: "Google · Gemini generateContent",
  }[protocol];
}

function providerProtocolLabel(protocol: string | null): string {
  const normalized = protocol === "openai" ? "openai_chat_completions"
    : protocol === "anthropic" ? "anthropic_messages"
      : protocol === "gemini" ? "gemini_generate_content"
        : protocol;
  if (normalized === null) return "未知";
  if (normalized === "openai_chat_completions" || normalized === "openai_responses"
    || normalized === "anthropic_messages" || normalized === "gemini_generate_content") {
    return protocolName(normalized);
  }
  return protocol ?? "未知";
}

function renderSingleProviderCard(provider: Provider): HTMLElement {
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
  const endpointText = document.createElement("span");
  endpointText.className = "provider-endpoint-text";
  endpointText.textContent = provider.models_endpoint;

  const copyButton = document.createElement("button");
  copyButton.type = "button";
  copyButton.className = "copy-endpoint-btn";
  copyButton.title = "复制接口地址";
  copyButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
  copyButton.addEventListener("click", () => {
    navigator.clipboard.writeText(provider.models_endpoint).then(() => {
      const originalHtml = copyButton.innerHTML;
      copyButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;
      setTimeout(() => { copyButton.innerHTML = originalHtml; }, 2000);
    });
  });

  const endpoint = document.createElement("code");
  endpoint.className = "provider-endpoint";
  endpoint.title = provider.models_endpoint;
  endpoint.append(endpointText, copyButton);
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
  count.textContent = `${providerUpstreams.length} 个上游模型`;
  providerMeta.append(protocol, count);
  heading.append(identity, providerMeta);

  const providerActions = document.createElement("div");
  providerActions.className = "provider-actions";
  const providerEditActions = document.createElement("div");
  providerEditActions.className = "provider-edit-actions";
  const manage = document.createElement("button");
  manage.type = "button";
  manage.className = "secondary compact-button";
  manage.textContent = "编辑上游服务";
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
  providerEditActions.append(manage, removeProviderButton);

  const providerTestActions = document.createElement("div");
  providerTestActions.className = "provider-test-actions";
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
    : "测试全部模型入口";
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
    testSummary.classList.add(failedVirtualModels.length > 0 ? "error" : "success");
    testSummary.textContent = `${passed}/${allVirtualModels.length} 通过`;
    testSummary.title = `最近测试：${formatActivityTime(testSession.completedAt).label} · ${passed} 通过 · ${failedVirtualModels.length} 失败`;
    providerTestActions.append(testSummary);
  }
  providerTestActions.append(testAllModels);
  providerActions.append(providerEditActions, providerTestActions);

  const models = document.createElement("div");
  models.className = "provider-models";
  if (modelLinks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = "尚未接入模型入口";
    models.append(empty);
  } else {
    const modelsHeader = document.createElement("div");
    modelsHeader.className = "provider-models-header";
    for (const label of ["上游模型", "模型能力", "模型入口"]) {
      const column = document.createElement("span");
      column.textContent = label;
      modelsHeader.append(column);
    }
    models.append(modelsHeader);

    for (const upstream of providerUpstreams) {
      const virtualModels = modelLinks
        .filter((link) => link.upstream.id === upstream.id)
        .map((link) => link.virtualModel);
      if (virtualModels.length > 0) {
        models.append(providerModelGroup(upstream, virtualModels));
      }
    }
  }

  card.append(heading, providerActions, models);
  return card;
}

function renderProviders(): void {
  providerCount.textContent = `${config.providers.length} 个服务`;
  providerList.replaceChildren();
  renderReadiness();

  if (config.providers.length === 0) {
    activeProviderTabId = null;
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "还没有上游服务。添加连接后即可获取并选择模型。";
    providerList.append(empty);
    return;
  }

  if (!activeProviderTabId || !config.providers.some((p) => p.id === activeProviderTabId)) {
    activeProviderTabId = config.providers[0].id;
  }

  const tabsBar = document.createElement("div");
  tabsBar.className = "provider-tabs-bar";

  for (const provider of config.providers) {
    const tabCard = document.createElement("button");
    tabCard.type = "button";
    const isActive = provider.id === activeProviderTabId;
    tabCard.className = `provider-tab-card${isActive ? " active" : ""}`;

    const providerUpstreams = config.upstream_models.filter(
      (upstream) => upstream.provider_id === provider.id,
    );
    const modelLinksCount = config.virtual_models.filter((virtualModel) => {
      return providerUpstreams.some((u) => u.id === virtualModel.upstream_model_id);
    }).length;

    const icon = document.createElement("span");
    icon.className = "provider-tab-icon";
    icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>`;

    const title = document.createElement("span");
    title.className = "provider-tab-title";
    title.textContent = provider.name;

    const badge = document.createElement("span");
    badge.className = "provider-tab-badge";
    badge.textContent = `${modelLinksCount}`;

    tabCard.append(icon, title, badge);
    tabCard.addEventListener("click", () => {
      if (activeProviderTabId !== provider.id) {
        activeProviderTabId = provider.id;
        renderProviders();
      }
    });
    tabsBar.append(tabCard);
  }

  providerList.append(tabsBar);

  const activeProvider = config.providers.find((p) => p.id === activeProviderTabId) ?? config.providers[0];
  const activeCard = renderSingleProviderCard(activeProvider);
  providerList.append(activeCard);
}

function providerModelGroup(
  upstream: UpstreamModel,
  virtualModels: VirtualModel[],
): HTMLElement {
  const item = document.createElement("article");
  item.className = "provider-model-item";

  const main = document.createElement("div");
  main.className = "provider-model-main";
  const name = document.createElement("h4");
  name.textContent = upstream.display_name;
  main.append(name);

  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("图像输入"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("工具调用"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("思考档位"));
  }

  const variants = document.createElement("div");
  variants.className = "provider-model-variants-inline";
  const sortedVirtualModels = sortVirtualModelsByReasoningLevel(virtualModels);
  for (const virtualModel of sortedVirtualModels) {
    const variant = document.createElement("div");
    variant.className = "model-variant-pill provider-model-variant";
    variant.dataset.virtualModelId = virtualModel.id;
    variant.title = virtualModel.display_name;

    const label = document.createElement("span");
    label.className = "model-variant-label";
    label.textContent = virtualModel.default_reasoning_level
      ? reasoningLevelLabel(virtualModel.default_reasoning_level)
      : "Default";

    const connectionResult = document.createElement("span");
    connectionResult.className = "connection-result";
    connectionResult.setAttribute("role", "status");
    connectionResult.setAttribute("aria-live", "polite");
    connectionResult.hidden = true;
    const existingState = connectionTestResults.get(virtualModel.id);
    if (existingState) renderConnectionTestState(connectionResult, existingState);

    variant.append(label, connectionResult);
    variants.append(variant);
  }

  item.append(main, capabilities, variants);
  return item;
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
  target.title = state.message;
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

  const concurrency = Math.min(3, virtualModels.length);
  await Promise.all(Array.from({ length: concurrency }, worker));

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
  return `无法删除：模型入口“${source.display_name}”仍将“${removed?.display_name ?? source.fallback_virtual_model_id}”用作备用模型。请先调整 fallback。`;
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
  badge.title = label;
  let icon = "";
  if (label === "图像输入") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`;
  } else if (label === "工具调用") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`;
  } else if (label === "思考档位") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`;
  }
  const shortLabels: Record<string, string> = {
    图像输入: "图像",
    工具调用: "工具",
    思考档位: "思考",
  };
  badge.innerHTML = `${icon}${shortLabels[label] ?? label}`;
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
    auto: "自定义",
  }[level];
}

function configurableReasoningLevels(protocol: ProviderProtocol): ConfigurableReasoningLevel[] {
  return protocol === "gemini_generate_content"
    ? ["low", "medium", "high"]
    : ["low", "medium", "high", "x_high", "max"];
}

const REASONING_LEVEL_ORDER: Record<ReasoningLevel, number> = {
  off: 0,
  low: 1,
  medium: 2,
  high: 3,
  x_high: 4,
  max: 5,
  auto: 6,
};

function sortReasoningLevels<T extends ReasoningLevel>(levels: Iterable<T>): T[] {
  return [...levels].sort(
    (a, b) => (REASONING_LEVEL_ORDER[a] ?? 99) - (REASONING_LEVEL_ORDER[b] ?? 99),
  );
}

function sortVirtualModelsByReasoningLevel(virtualModels: VirtualModel[]): VirtualModel[] {
  return [...virtualModels].sort((a, b) => {
    const orderA = a.default_reasoning_level ? (REASONING_LEVEL_ORDER[a.default_reasoning_level] ?? 99) : -1;
    const orderB = b.default_reasoning_level ? (REASONING_LEVEL_ORDER[b.default_reasoning_level] ?? 99) : -1;
    return orderA - orderB;
  });
}

function catalogReasoningLevelsForModel(
  model: ProviderCatalogModel,
  protocol: ProviderProtocol,
  existingUpstream?: UpstreamModel,
): ConfigurableReasoningLevel[] {
  if (model.reasoning?.supported === false && !existingUpstream) return [];
  const explicit = (model.reasoning?.levels ?? []).filter(
    (level): level is ConfigurableReasoningLevel =>
      configurableReasoningLevels(protocol).includes(level as ConfigurableReasoningLevel),
  );
  if (explicit.length > 0) return sortReasoningLevels([...new Set(explicit)]);
  const existing = existingUpstream
    ? (Object.keys(existingUpstream.capabilities.reasoning.levels) as ReasoningLevel[]).filter(
        (level): level is ConfigurableReasoningLevel =>
          configurableReasoningLevels(protocol).includes(level as ConfigurableReasoningLevel),
      )
    : [];
  return sortReasoningLevels(
    existing.length > 0 ? [...new Set(existing)] : configurableReasoningLevels(protocol),
  );
}



function catalogReasoningMetadataLabel(model: ProviderCatalogModel): string | null {
  const metadata = model.reasoning;
  if (!metadata) return null;
  if (metadata.supported === false) return "思考：不支持";
  const levels = (metadata.levels ?? []).filter((level) => level !== "off" && level !== "auto");
  if (levels.length > 0) return `思考：${sortReasoningLevels(levels).map(reasoningLevelLabel).join(" · ")}`;
  if (metadata.supported === true) return "思考：支持（等级未声明）";
  return "思考：未声明";
}

function customReasoningValueFromUpstream(upstream: UpstreamModel): string | null {
  const mapping = upstream.capabilities.reasoning.levels.auto;
  if (!mapping) return null;
  if (mapping.kind === "effort" || mapping.kind === "native_level") return mapping.value;
  if (mapping.kind === "budget_tokens") return String(mapping.value);
  return null;
}

function reasoningLevelsForVirtualModels(
  protocol: ProviderProtocol,
  virtualModels: VirtualModel[],
): Set<ConfigurableReasoningLevel> {
  const configurable = new Set<ReasoningLevel>(configurableReasoningLevels(protocol));
  const levels = virtualModels.flatMap((virtualModel) => {
    const level = virtualModel.default_reasoning_level;
    return level && configurable.has(level)
      ? [level as ConfigurableReasoningLevel]
      : [];
  });
  return new Set(sortReasoningLevels(levels));
}

function customReasoningMapping(protocol: ProviderProtocol, value: string): ReasoningMapping | null {
  const normalized = value.trim();
  if (!normalized) return null;
  if (protocol === "openai_chat_completions" || protocol === "openai_responses") {
    return { kind: "effort", value: normalized };
  }
  const budgetTokens = Number(normalized);
  if (!Number.isInteger(budgetTokens) || budgetTokens < 1024) return null;
  return { kind: "budget_tokens", value: budgetTokens };
}


function reasoningLevels(protocol: ProviderProtocol): Partial<Record<ReasoningLevel, ReasoningMapping>> {
  if (protocol === "anthropic_messages") {
    return {
      low: { kind: "budget_tokens", value: 1024 },
      medium: { kind: "budget_tokens", value: 4096 },
      high: { kind: "budget_tokens", value: 8192 },
      x_high: { kind: "budget_tokens", value: 16384 },
      max: { kind: "budget_tokens", value: 32768 },
    };
  }
  if (protocol === "gemini_generate_content") {
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



async function removeProvider(providerId: string, button: HTMLButtonElement): Promise<void> {
  await withBusy(button, async () => {
    const upstreamIds = new Set(
      config.upstream_models
        .filter((item) => item.provider_id === providerId)
        .map((item) => item.id),
    );
    const remainingProviders = config.providers.filter((item) => item.id !== providerId);
    if (activeProviderTabId === providerId) {
      activeProviderTabId = remainingProviders.length > 0 ? remainingProviders[0].id : null;
    }
    await persistConfig({
      proxy_port: config.proxy_port,
      providers: remainingProviders,
      upstream_models: config.upstream_models.filter((item) => item.provider_id !== providerId),
      virtual_models: config.virtual_models.filter(
        (item) => !upstreamIds.has(item.upstream_model_id),
      ),
    });
    showNotice("上游服务及其接入模型已删除");
  });
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

function protocolDescription(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: "适用于 /v1/chat/completions 接口，支持 CPA、Sub2API 及主流 OpenAI 兼容服务网关。",
    openai_responses: "适用于 OpenAI Responses API 兼容接口（/v1/responses），请勿误选为 Chat Completions。",
    anthropic_messages: "适用于 /v1/messages 接口，支持 Anthropic 官方 API 及兼容 Messages API 的中转服务。",
    gemini_generate_content: "适用于 Google Gemini 原生 API（:generateContent），支持 /v1beta/models 接口。",
  }[protocol];
}

function updateProtocolHelp(): void {
  const protocol = element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
  element<HTMLElement>("#protocol-help").textContent = protocolDescription(protocol);
}

function selectedProtocol(): ProviderProtocol {
  return element<HTMLSelectElement>("#protocol").value as ProviderProtocol;
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

function providerFromForm(): Provider {
  const protocol = selectedProtocol();
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
  catalogReasoningLevelsByModel = new Map();
  catalogCustomReasoningByModel = new Map();
  catalogVisionEnabledModelIds = new Set();
  catalogToolsEnabledModelIds = new Set();
  catalogReasoningEnabledModelIds = new Set();
  changedCatalogCapabilityModelIds = new Set();
  changedCatalogReasoningModelIds = new Set();
  legacyCatalogModelIds = new Set();
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
  providerDirtyBadge.hidden = true;
  invalidatePendingProviderSave();
  refreshProviderEditorControls();
}

async function closeProviderEditor(force = false): Promise<boolean> {
  if (!force && !(await confirmDiscardProviderChanges())) return false;
  const returnFocus = providerEditorReturnFocus;
  providerEditorReturnFocus = null;
  providerFormPanel.hidden = true;
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

async function openProviderEditor(providerId: string | null = null): Promise<void> {
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
    ? config.providers.find((item) => item.id === providerId)
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
  catalogReasoningLevelsByModel = new Map(catalogModels.map((model) => {
    const upstream = existingUpstreamsByModelId.get(model.id);
    if (!upstream) return [model.id, new Set<ConfigurableReasoningLevel>()];
    const virtualModels = config.virtual_models.filter(
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
  catalogResults.hidden = false;
  element<HTMLElement>("#catalog-status").textContent = legacyCatalogModelIds.size > 0
    ? `目录获取成功 · ${fetched.length} 个模型 · ${legacyCatalogModelIds.size} 个已配置模型未返回`
    : `目录获取成功 · ${fetched.length} 个模型`;
  renderCatalogModels();
  catalogResults.scrollIntoView({ behavior: "smooth", block: "nearest" });
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

function openReasoningModal(model: ProviderCatalogModel): void {
  activeReasoningModel = model;
  const existingUpstream = editingProviderId
    ? config.upstream_models.find(
        (item) => item.provider_id === editingProviderId && item.upstream_model_id === model.id,
      )
    : undefined;
  const currentLevels = catalogReasoningLevelsByModel.get(model.id) ?? new Set<ConfigurableReasoningLevel>();
  draftReasoningLevels = new Set(sortReasoningLevels(currentLevels));

  reasoningModalTitle.textContent = `推理强度配置 · ${model.displayName}`;
  reasoningModalLevelsContainer.replaceChildren();

  const supportedLevels = catalogReasoningLevelsForModel(model, selectedProtocol(), existingUpstream);
  for (const level of supportedLevels) {
    const row = document.createElement("div");
    row.className = "reasoning-modal-level-row";

    const label = document.createElement("label");
    label.className = "check-label";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = draftReasoningLevels.has(level);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) draftReasoningLevels.add(level);
      else draftReasoningLevels.delete(level);
    });
    const text = document.createElement("span");
    text.textContent = reasoningLevelLabel(level);
    label.append(checkbox, text);

    const testArea = document.createElement("div");
    testArea.className = "reasoning-level-test-area";
    const result = document.createElement("span");
    result.className = "reasoning-level-test-result";
    result.setAttribute("role", "status");

    const testBtn = document.createElement("button");
    testBtn.type = "button";
    testBtn.className = "secondary compact-button";
    testBtn.textContent = "测试";
    testBtn.addEventListener("click", () => {
      void withProviderEditorBusy(testBtn, async () => {
        result.className = "reasoning-level-test-result pending";
        result.textContent = "测试中…";
        const response = await invoke<ModelConnectionTestResult>(
          "test_provider_model_connection",
          {
            provider: providerFromForm(),
            upstreamModelId: model.id,
            reasoningLevel: level,
            customReasoningValue: null,
          },
        );
        if (response.success) {
          result.className = "reasoning-level-test-result success";
          result.textContent = `通过 · ${response.durationMs} ms`;
        } else {
          result.className = "reasoning-level-test-result error";
          result.textContent = `失败 · ${response.message}`;
        }
        result.title = response.message ?? "";
      }, "测试中…");
    });

    testArea.append(result, testBtn);
    row.append(label, testArea);
    reasoningModalLevelsContainer.append(row);
  }

  reasoningModal.hidden = false;
}

function closeReasoningModal(): void {
  reasoningModal.hidden = true;
  activeReasoningModel = null;
}

confirmReasoningModalButton.addEventListener("click", () => {
  if (!activeReasoningModel) return;
  const modelId = activeReasoningModel.id;
  if (draftReasoningLevels.size > 0) {
    catalogReasoningEnabledModelIds.add(modelId);
    catalogReasoningLevelsByModel.set(modelId, new Set(sortReasoningLevels(draftReasoningLevels)));
  } else {
    catalogReasoningEnabledModelIds.delete(modelId);
    catalogReasoningLevelsByModel.delete(modelId);
  }
  changedCatalogReasoningModelIds.add(modelId);
  setProviderEditorDirty(true);
  renderCatalogModels();
  closeReasoningModal();
});

cancelReasoningModalButton.addEventListener("click", closeReasoningModal);
closeReasoningModalButton.addEventListener("click", closeReasoningModal);
reasoningModalBackdrop.addEventListener("click", closeReasoningModal);

function renderCatalogModels(): void {
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  catalogModelList.replaceChildren();

  for (const model of visibleModels) {
    const row = document.createElement("div");
    const selected = selectedCatalogModelIds.has(model.id);
    const existingUpstream = editingProviderId
      ? config.upstream_models.find(
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
          const response = await invoke<ModelConnectionTestResult>(
            "test_provider_model_connection",
            {
              provider,
              upstreamModelId: model.id,
              reasoningLevel: testCase.reasoningLevel,
              customReasoningValue: null,
            },
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
  activeProviderTabId = plan.provider.id;
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
    showNotice("请确认保存并移除列出的模型入口", "error");
    return;
  }
  await executeProviderSave(plan);
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

function formatNumberCompact(num: number | null): string {
  if (num === null || num === undefined) return "—";
  if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`;
  if (num >= 10_000) return `${(num / 1_000).toFixed(1)}k`;
  return num.toLocaleString();
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

    const mainGroup = document.createElement("div");
    mainGroup.className = "activity-item-main";

    const timestamp = document.createElement("time");
    const formattedTime = formatActivityTime(item.timestampMs);
    timestamp.className = "activity-time";
    timestamp.textContent = formattedTime.label;
    if (formattedTime.dateTime) timestamp.dateTime = formattedTime.dateTime;

    const path = document.createElement("div");
    path.className = "activity-path";

    const reqCode = document.createElement("code");
    reqCode.textContent = context.requestedName;
    reqCode.title = item.requestedVirtualModelId ?? item.virtualModelId;

    const arrow = document.createElement("span");
    arrow.className = "activity-path-arrow";
    arrow.textContent = "──➔";

    const targetCode = document.createElement("span");
    targetCode.className = "activity-path-target";
    targetCode.textContent = `${context.providerName} (${context.upstreamName})`;
    targetCode.title = `实际上游: ${context.upstreamName} / 协议: ${providerProtocolLabel(item.providerProtocol)}`;

    path.append(reqCode, arrow, targetCode);
    mainGroup.append(timestamp, path);

    const statusGroup = document.createElement("div");
    statusGroup.className = "activity-status-group";

    const latency = document.createElement("span");
    const speedClass = item.durationMs < 1000 ? "fast" : item.durationMs < 4000 ? "medium" : "slow";
    latency.className = `activity-latency ${speedClass}`;
    latency.textContent = formatDuration(item.durationMs);

    const status = document.createElement("span");
    status.className = `status-pill ${failed ? "error" : item.fallbackSucceeded ? "accent" : "success"}`;
    const httpText = item.statusCode > 0 ? String(item.statusCode) : "无响应";
    status.textContent = failed
      ? `${httpText} · 失败`
      : item.fallbackSucceeded
        ? `${httpText} · Fallback`
        : `${httpText} OK`;

    statusGroup.append(latency, status);
    heading.append(mainGroup, statusGroup);

    const pillsRow = document.createElement("div");
    pillsRow.className = "activity-pills-row";

    const providerPill = document.createElement("span");
    providerPill.className = "activity-pill";
    providerPill.textContent = `${context.providerName} / ${providerProtocolLabel(item.providerProtocol)}`;

    const typePill = document.createElement("span");
    typePill.className = "activity-pill";
    typePill.textContent = item.stream ? "流式" : "非流式";

    const countPill = document.createElement("span");
    countPill.className = "activity-pill";
    countPill.textContent = `${item.messageCount} 消息 · ${item.toolCount} 工具`;

    pillsRow.append(providerPill, typePill, countPill);

    if (item.promptTokens !== null || item.completionTokens !== null) {
      const tokenPill = document.createElement("span");
      tokenPill.className = "activity-pill accent";
      const pFormat = formatNumberCompact(item.promptTokens);
      const cFormat = formatNumberCompact(item.completionTokens);
      tokenPill.textContent = `TOKEN: ${pFormat} 输入 · ${cFormat} 输出`;
      tokenPill.title = `输入 ${item.promptTokens ?? "—"} · 输出 ${item.completionTokens ?? "—"}`;
      pillsRow.append(tokenPill);
    }

    if (item.fallbackAttempted) {
      const fbPill = document.createElement("span");
      fbPill.className = `activity-pill ${item.fallbackSucceeded ? "accent" : "warning"}`;
      fbPill.textContent = item.fallbackSucceeded ? "Fallback 降级成功" : "Fallback 降级失败";
      pillsRow.append(fbPill);
    }

    card.append(heading, pillsRow);

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
      copy.textContent = "复制错误诊断";
      copy.addEventListener("click", () => {
        const text = [
          `时间: ${formattedTime.label}`,
          `请求模型: ${context.requestedName}`,
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

  if (nearTop) {
    activityList.scrollTop = 0;
  } else {
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


function renderApp(status: AppStatus): void {
  latestAppStatus = status;
  appStatusLoadFailed = false;
  const state = element<HTMLSpanElement>("#app-state");
  const detail = element<HTMLParagraphElement>("#app-detail");
  state.textContent = status.appRunning ? "运行中" : status.installed ? "已安装" : "未安装";
  state.className = `status-pill ${status.appRunning ? "success" : status.installed ? "neutral" : "error"}`;
  detail.textContent = status.appRunning
    ? "Antigravity App 正在运行"
    : status.installed
      ? "Antigravity App 已安装，当前未运行"
      : "未找到 Antigravity App";

  const integrationState = element<HTMLSpanElement>("#app-integration-state");
  const integrationDetail = element<HTMLParagraphElement>("#app-integration-detail");
  const visibleIntegrationState = displayIntegrationState(status.integrationState, status.configurationState);
  integrationState.textContent = integrationStateLabel(visibleIntegrationState);
  integrationState.className = `status-pill ${integrationStateClass(visibleIntegrationState)}`;
  integrationDetail.textContent = clientStatusMessage(
    status.integrationState,
    status.configurationState,
    status.configurationMessage,
  );

  const enableAppBtn = element<HTMLButtonElement>("#enable-app-integration");
  const launchAppBtn = element<HTMLButtonElement>("#launch-app");
  const disableAppBtn = element<HTMLButtonElement>("#disable-app-integration");

  enableAppBtn.hidden = !status.canEnableIntegration;
  launchAppBtn.hidden = !status.canLaunchApp || status.appRunning;
  disableAppBtn.hidden = !status.canDisableIntegration;

  enableAppBtn.textContent = status.appRunning ? "启用并重启" : "启用模型";
  launchAppBtn.textContent = "启动 App";
  disableAppBtn.textContent = status.appRunning ? "停用并重启" : "停用模型";

  setButtonUnavailable(enableAppBtn, !status.canEnableIntegration);
  setButtonUnavailable(launchAppBtn, !status.canLaunchApp);
  setButtonUnavailable(disableAppBtn, !status.canDisableIntegration);
  renderReadiness();
}

async function refreshIde(): Promise<void> {
  try {
    renderIde(await invoke<IdeStatus>("discover_ide"));
  } catch (error) {
    ideStatusLoadFailed = true;
    renderReadiness();
    throw error;
  }
}

async function refreshApp(): Promise<void> {
  try {
    renderApp(await invoke<AppStatus>("discover_app"));
  } catch (error) {
    appStatusLoadFailed = true;
    renderReadiness();
    throw error;
  }
}

let hostRefreshInFlight: Promise<void> | null = null;

async function refreshHostStatuses(): Promise<void> {
  if (hostRefreshInFlight) return hostRefreshInFlight;
  const task = Promise.allSettled([refreshIde(), refreshApp()]).then(() => undefined);
  hostRefreshInFlight = task;
  try {
    await task;
  } finally {
    if (hostRefreshInFlight === task) hostRefreshInFlight = null;
  }
}

async function initialize(): Promise<void> {
  const [configResult, proxyResult, ideResult, appResult, activityResult] = await Promise.allSettled([
    invoke<AppConfig>("get_config"),
    invoke<ProxyStatus>("proxy_status"),
    invoke<IdeStatus>("discover_ide"),
    invoke<AppStatus>("discover_app"),
    invoke<ActivityItem[]>("get_activity_log"),
  ]);
  const failures: string[] = [];
  proxyStatusLoadFailed = proxyResult.status === "rejected";
  ideStatusLoadFailed = ideResult.status === "rejected";
  appStatusLoadFailed = appResult.status === "rejected";
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
  if (appResult.status === "fulfilled") renderApp(appResult.value);
  else {
    failures.push("App 状态");
    element<HTMLElement>("#app-state").textContent = "读取失败";
    element<HTMLElement>("#app-integration-state").textContent = "读取失败";
    element<HTMLElement>("#app-integration-detail").textContent = `状态读取失败：${errorMessage(appResult.reason)}`;
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
  renderReadiness();
  if (failures.length > 0) {
    showNotice(`部分状态读取失败：${failures.join("、")}`, "error");
  }
}

async function startProxy(): Promise<void> {
  const status = await invoke<ProxyStatus>("start_proxy");
  renderProxy(status);
  await refreshIde();
  await refreshApp();
  showNotice("服务已启动");
}

stopProxyButton.addEventListener("click", () => void withBusy(stopProxyButton, async () => {
  renderProxy(await invoke<ProxyStatus>("stop_proxy"));
  const results = await Promise.allSettled([refreshIde(), refreshApp()]);
  if (results.some((result) => result.status === "rejected")) {
    showNotice("服务已停止，但应用状态刷新失败，请手动刷新", "error");
  } else {
    showNotice("服务已停止；已启用的模型暂时无法使用");
  }
}));

element<HTMLButtonElement>("#refresh-ide").addEventListener("click", (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  void withBusy(button, refreshIde);
});

const enableAppButton = element<HTMLButtonElement>("#enable-app-integration");
const launchAppButton = element<HTMLButtonElement>("#launch-app");
const disableAppButton = element<HTMLButtonElement>("#disable-app-integration");

enableAppButton.addEventListener("click", () => {
  void (async () => {
    const status = await withClientBusy(enableAppButton, "app", async () => {
      const isRunning = latestAppStatus?.appRunning ?? false;
      const confirmMsg = isRunning
        ? "启用模型后，App 会自动重启。是否继续？"
        : "启用模型后，App 就可以使用自定义模型。是否继续？";
      if (!await confirmHostAction(confirmMsg, "确认启用模型")) return null;

      showNotice("正在启用模型…");
      return invoke<AppStatus>("enable_app_integration");
    });
    if (status === null) return;
    if (status) {
      renderApp(status);
      showNotice(status.appRunning
        ? "App 已启用模型并完成重启"
        : "App 已启用模型，可以启动 App");
    } else if (latestAppStatus) {
      try {
        await refreshApp();
      } catch {
        // withClientBusy already reported the operation error.
      }
    }
  })();
});

launchAppButton.addEventListener("click", () => {
  void withClientBusy(launchAppButton, "app", async () => {
    await invoke<void>("launch_app");
    showNotice("已启动 App");
    window.setTimeout(() => void refreshApp().catch(() => undefined), 700);
  }, "启动中…");
});

disableAppButton.addEventListener("click", () => {
  void (async () => {
    const status = await withClientBusy(disableAppButton, "app", async () => {
      const isRunning = latestAppStatus?.appRunning ?? false;
      const confirmMsg = isRunning
        ? "停用模型后，App 会自动重启并恢复官方模型。是否继续？"
        : "停用模型后，App 下次启动时将使用官方模型。是否继续？";
      if (!await confirmHostAction(confirmMsg, "确认停用模型")) return null;

      showNotice("正在停用模型…");
      return invoke<AppStatus>("disable_app_integration");
    });
    if (status === null) return;
    if (status) {
      renderApp(status);
      showNotice(status.appRunning
        ? "App 已停用模型并完成重启"
        : "App 已停用模型");
    } else if (latestAppStatus) {
      try {
        await refreshApp();
      } catch {
        // withClientBusy already reported the operation error.
      }
    }
  })();
});

element<HTMLButtonElement>("#refresh-app").addEventListener("click", (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  void withBusy(button, refreshApp);
});

window.addEventListener("focus", () => {
  void refreshHostStatuses();
});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void refreshHostStatuses();
});

const copyProxyAddressBtn = document.querySelector("#copy-proxy-address");
if (copyProxyAddressBtn) {
  copyProxyAddressBtn.addEventListener("click", () => {
    const address = element<HTMLElement>("#proxy-address").textContent?.trim() ?? "";
    if (!address) return;
    const fullUrl = address.startsWith("http") ? address : `http://${address}`;
    navigator.clipboard.writeText(fullUrl).then(() => {
      showNotice(`已复制代理地址 ${fullUrl}`);
    }).catch((err) => {
      showNotice(`复制失败：${errorMessage(err)}`, "error");
    });
  });
}

const openIdeSettingsBtn = document.querySelector("#open-ide-settings");
if (openIdeSettingsBtn) {
  openIdeSettingsBtn.addEventListener("click", () => {
    const path = latestIdeStatus?.settingsPath || document.querySelector("#ide-settings-path-display")?.textContent?.trim();
    if (!path) {
      showNotice("配置文件路径未知", "error");
      return;
    }
    invoke<void>("open_path", { path })
      .then(() => {
        showNotice("已在系统默认编辑器中打开配置文件");
      })
      .catch((err) => {
        showNotice(`打开配置文件失败：${errorMessage(err)}`, "error");
      });
  });
}

const copyIdeSettingsPathBtn = document.querySelector("#copy-ide-settings-path");
if (copyIdeSettingsPathBtn) {
  copyIdeSettingsPathBtn.addEventListener("click", () => {
    const path = latestIdeStatus?.settingsPath || document.querySelector("#ide-settings-path-display")?.textContent?.trim();
    if (!path) return;
    navigator.clipboard.writeText(path).then(() => {
      showNotice(`已复制配置文件路径`);
    }).catch((err) => {
      showNotice(`复制失败：${errorMessage(err)}`, "error");
    });
  });
}

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

cancelProviderButton.addEventListener("click", () => {
  void closeProviderEditor();
});

enableIdeIntegrationButton.addEventListener("click", () => {
  void (async () => {
    const status = await withClientBusy(enableIdeIntegrationButton, "ide", async () => {
      const isRunning = latestIdeStatus?.ideRunning ?? false;
      const confirmMsg = isRunning
        ? "启用模型后，IDE 会自动重启。是否继续？"
        : "启用模型后，IDE 就可以使用自定义模型。是否继续？";
      if (!await confirmHostAction(confirmMsg, "确认启用模型")) return null;

      showNotice("正在启用模型…");
      return invoke<IdeStatus>("enable_ide_integration");
    });
    if (status === null) return;
    if (status) {
      renderIde(status);
      showNotice(status.ideRunning
        ? "IDE 已启用模型并完成重启"
        : "IDE 已启用模型，可以启动 IDE");
    } else if (latestIdeStatus) {
      try {
        await refreshIde();
      } catch {
        // withClientBusy already reported the operation error.
      }
    }
  })();
});

launchIdeButton.addEventListener("click", () => {
  void withClientBusy(launchIdeButton, "ide", async () => {
    await invoke<void>("launch_ide");
    showNotice("已启动 IDE");
    window.setTimeout(() => void refreshIde().catch(() => undefined), 700);
  }, "启动中…");
});

disableIdeIntegrationButton.addEventListener("click", () => {
  void (async () => {
    const status = await withClientBusy(disableIdeIntegrationButton, "ide", async () => {
      const isRunning = latestIdeStatus?.ideRunning ?? false;
      const confirmMsg = isRunning
        ? "停用模型后，IDE 会自动重启并恢复官方模型。是否继续？"
        : "停用模型后，IDE 下次启动时将使用官方模型。是否继续？";
      if (!await confirmHostAction(confirmMsg, "确认停用模型")) return null;

      showNotice("正在停用模型…");
      return invoke<IdeStatus>("disable_ide_integration");
    });
    if (status === null) return;
    if (status) {
      renderIde(status);
      showNotice(status.ideRunning
        ? "IDE 已停用模型并完成重启"
        : "IDE 已停用模型");
    } else if (latestIdeStatus) {
      try {
        await refreshIde();
      } catch {
        // withClientBusy already reported the operation error.
      }
    }
  })();
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



element<HTMLButtonElement>("#close-provider-modal").addEventListener("click", () => {
  void closeProviderEditor();
});

element<HTMLElement>("#provider-modal-backdrop").addEventListener("click", () => {
  void closeProviderEditor();
});

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

const tabTriggers = [...document.querySelectorAll<HTMLButtonElement>(".tab-trigger")];
const tabPanes = [...document.querySelectorAll<HTMLElement>(".tab-pane")];
const pageTitle = element<HTMLSpanElement>("#page-title-text");
const pageDescription = element<HTMLParagraphElement>("#page-description");
const tabCopy: Record<string, { title: string; description: string }> = {
  "tab-status": {
    title: "运行概览",
    description: "查看代理服务、IDE 和 App 的运行状态。",
  },
  "tab-models": {
    title: "模型管理",
    description: "管理 AI 上游服务及其接入 IDE / App 的模型与推理配置。",
  },
  "tab-activity": {
    title: "调用日志",
    description: "查看请求路由、Token 用量与失败详情。",
  },
};

// ==========================================================================
// Theme Manager & Enhancements Initialization
// ==========================================================================
function initThemeManager(): void {
  const savedTheme = localStorage.getItem("agy_theme") || "light";
  applyTheme(savedTheme);

  const toggleBtn = document.querySelector("#minimal-theme-toggle");

  toggleBtn?.addEventListener("click", () => {
    const currentTheme = document.documentElement.getAttribute("data-theme") || "light";
    const nextTheme = currentTheme === "dark" ? "light" : "dark";
    localStorage.setItem("agy_theme", nextTheme);
    applyTheme(nextTheme);
  });
}

function applyTheme(theme: string): void {
  let effectiveTheme = theme;
  if (theme === "system") {
    effectiveTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  document.documentElement.setAttribute("data-theme", effectiveTheme);

  const toggleBtn = document.querySelector("#minimal-theme-toggle");
  if (toggleBtn) {
    const sunIcon = toggleBtn.querySelector(".icon-sun");
    const moonIcon = toggleBtn.querySelector(".icon-moon");
    if (sunIcon) sunIcon.toggleAttribute("hidden", effectiveTheme === "dark");
    if (moonIcon) moonIcon.toggleAttribute("hidden", effectiveTheme !== "dark");
  }
}

initThemeManager();

async function switchTab(targetId: string): Promise<void> {
  const currentPane = tabPanes.find((pane) => pane.classList.contains("active"));
  if (currentPane?.id === targetId) return;
  if (!providerFormPanel.hidden) {
    if (!(await confirmDiscardProviderChanges())) return;
    void closeProviderEditor(true);
  }

  for (const trigger of tabTriggers) {
    const active = trigger.dataset.target === targetId;
    trigger.classList.toggle("active", active);
    trigger.setAttribute("aria-current", active ? "page" : "false");
  }
  for (const pane of tabPanes) {
    pane.classList.toggle("active", pane.id === targetId);
  }
  const copy = tabCopy[targetId];
  if (copy) {
    pageTitle.textContent = copy.title;
    pageDescription.textContent = copy.description;
  }
  window.scrollTo({ top: 0, behavior: "smooth" });
}

for (const trigger of tabTriggers) {
  trigger.addEventListener("click", () => {
    const targetId = trigger.dataset.target;
    if (targetId) switchTab(targetId);
  });
}

setupProviderPresets();
void initialize();
