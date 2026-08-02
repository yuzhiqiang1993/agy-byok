import { element } from "../utils/domUtils";
import { store } from "../store/appStore";
import { integrationStateLabel, displayIntegrationState, clientConfigurationReady, clientReady } from "../utils/displayUtils";

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
  setReadinessStep(
    "#readiness-cli",
    "#readiness-cli-value",
    cliStatusLoadFailed ? "attention" : latestCliStatus === null ? "pending" : cliReady ? "ready" : "attention",
    cliStatusLoadFailed
      ? "读取失败"
      : latestCliStatus === null
        ? "检查中"
        : !latestCliStatus.installed
          ? "未安装"
          : integrationStateLabel(displayIntegrationState(
              latestCliStatus.integrationState,
              latestCliStatus.configurationState,
            )),
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  if (modelCountValue === 0) {
    title.textContent = "先添加要使用的模型";
    detail.textContent = "添加模型后，即可在 IDE、App 或 CLI 中接入使用。";
  } else if (proxyStatusLoadFailed || ideStatusLoadFailed || appStatusLoadFailed || cliStatusLoadFailed) {
    title.textContent = "部分运行状态读取失败";
    detail.textContent = "请使用对应客户端卡片的刷新操作重试。";
  } else if (latestProxyStatus === null || latestIdeStatus === null || latestAppStatus === null || latestCliStatus === null) {
    title.textContent = "正在确认运行状态…";
    detail.textContent = `已设置 ${modelCountValue} 个模型。`;
  } else if (!proxyRunning) {
    title.textContent = "模型已准备好，请启动代理";
    detail.textContent = "在右侧卡片点击“启动代理”后，已接入的客户端即可使用自定义模型。";
  } else if (!ideReady && !appReady && !cliReady) {
    title.textContent = "选择要接入的客户端";
    detail.textContent = "在下方卡片中点击“接入模型”即可为对应的 IDE、App 或 CLI 开启自定义模型。";
  } else {
    const enabledClients = [
      ideReady ? "IDE" : null,
      appReady ? "App" : null,
      cliReady ? "CLI" : null,
    ].filter((item): item is string => item !== null);
    title.textContent = `${enabledClients.join("、")} 已接入模型`;
    detail.textContent = "现在可以直接在已接入的客户端中使用自定义模型。";
  }
}
