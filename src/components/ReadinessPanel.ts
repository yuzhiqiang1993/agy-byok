import { element } from "../utils/domUtils";
import { store } from "../store/appStore";
import { clientConfigurationReady, clientReady } from "../utils/displayUtils";

function setReadinessStep(
  selector: string,
  valueSelector: string,
  state: "pending" | "ready" | "attention",
  value: string,
): void {
  element<HTMLLIElement>(selector).dataset.state = state;
  element<HTMLElement>(valueSelector).textContent = value;
}

export function renderReadiness(): void {
  const modelCountValue = store.config?.virtual_models.length ?? 0;
  const proxyRunning = store.proxyStatus?.state === "running";

  const latestProxyStatus = store.proxyStatus;
  const latestIdeStatus = store.ideStatus;
  const latestAppStatus = store.appStatus;
  const latestCliStatus = store.cliStatus;

  const proxyStatusLoadFailed = store.proxyStatusLoadFailed;
  const ideStatusLoadFailed = store.ideStatusLoadFailed;
  const appStatusLoadFailed = store.appStatusLoadFailed;
  const cliStatusLoadFailed = store.cliStatusLoadFailed;

  const ideReady = latestIdeStatus
    ? latestIdeStatus.compatible
      && clientReady(latestIdeStatus.integrationState)
      && clientConfigurationReady(latestIdeStatus.configurationState, proxyRunning)
    : false;
  const appReady = latestAppStatus
    ? latestAppStatus.installed
      && latestAppStatus.integrationState === "managed"
      && clientConfigurationReady(latestAppStatus.configurationState, proxyRunning)
    : false;
  const cliReady = latestCliStatus
    ? latestCliStatus.installed
      && latestCliStatus.integrationState === "managed"
      && clientConfigurationReady(latestCliStatus.configurationState, proxyRunning)
    : false;

  const enabledClients = [
    ideReady ? "IDE" : null,
    appReady ? "App" : null,
    cliReady ? "CLI" : null,
  ].filter((item): item is string => item !== null);
  const entryStatusesLoadFailed = ideStatusLoadFailed || appStatusLoadFailed || cliStatusLoadFailed;
  const entryStatusesLoading = latestIdeStatus === null || latestAppStatus === null || latestCliStatus === null;

  setReadinessStep(
    "#readiness-models",
    "#readiness-models-value",
    modelCountValue > 0 ? "ready" : "attention",
    modelCountValue > 0 ? `${modelCountValue} 个模型` : "待添加",
  );
  setReadinessStep(
    "#readiness-proxy",
    "#readiness-proxy-value",
    proxyStatusLoadFailed ? "attention" : latestProxyStatus === null ? "pending" : proxyRunning ? "ready" : "attention",
    proxyStatusLoadFailed ? "读取失败" : latestProxyStatus === null ? "检查中" : proxyRunning ? "运行中" : "待启动",
  );
  setReadinessStep(
    "#readiness-entry",
    "#readiness-entry-value",
    entryStatusesLoadFailed
      ? "attention"
      : entryStatusesLoading
        ? "pending"
        : enabledClients.length > 0
          ? "ready"
          : "attention",
    entryStatusesLoadFailed
      ? "读取失败"
      : entryStatusesLoading
        ? "检查中"
        : enabledClients.length > 0
          ? `已启用 ${enabledClients.join("、")}`
          : "选择入口",
  );
  setReadinessStep(
    "#readiness-restore",
    "#readiness-restore-value",
    "ready",
    "随时可用",
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  if (modelCountValue === 0) {
    title.textContent = "第 1 步：先配置上游和模型";
    detail.textContent = "进入“模型管理”，添加上游服务，获取模型列表并保存需要使用的模型。";
  } else if (proxyStatusLoadFailed || entryStatusesLoadFailed) {
    title.textContent = "部分运行状态读取失败";
    detail.textContent = "请使用对应入口卡片的刷新操作重试。";
  } else if (latestProxyStatus === null || entryStatusesLoading) {
    title.textContent = "正在确认运行状态…";
    detail.textContent = `已配置 ${modelCountValue} 个模型，正在检查代理和入口状态。`;
  } else if (!proxyRunning) {
    title.textContent = "第 2 步：启动本地代理";
    detail.textContent = "模型配置已完成。启动代理后，IDE、App 或 CLI 才能使用这些模型。";
  } else if (enabledClients.length === 0) {
    title.textContent = "第 3 步：选择要启用的入口";
    detail.textContent = "在下方选择 IDE、App 或 CLI，点击“启用代理模式”。入口可以单独启用，也可以同时启用多个。";
  } else {
    title.textContent = "代理模式已启用";
    detail.textContent = "已启用的入口可以使用自定义模型。任何时候都可以恢复对应入口的官方配置。";
  }
}
