import "./styles.css";
import { store } from "./store/appStore";
import { configService } from "./services/configService";
import { proxyService } from "./services/proxyService";
import { hostService } from "./services/hostService";
import { activityService } from "./services/activityService";

import { setupProxyCard, renderProxy } from "./components/ProxyCard";
import { setupIdeCard, renderIde } from "./components/IdeCard";
import { setupAppCard, renderApp } from "./components/AppCard";
import { setupCliCard, renderCli } from "./components/CliCard";
import { renderReadiness } from "./components/ReadinessPanel";
import { renderProviders } from "./components/ProviderList";
import { setupProviderEditor } from "./components/ProviderEditor";
import { setActivityItems, setupActivityList } from "./components/ActivityList";
import { showNotice, setupNoticeBar } from "./components/NoticeBar";
import { initThemeManager } from "./components/ThemeManager";
import { setupTabManager } from "./components/TabManager";
import { errorMessage } from "./utils/domUtils";
import { setupReasoningModal } from "./components/ReasoningModal";

setupNoticeBar();
setupProxyCard();
setupIdeCard();
setupAppCard();
setupCliCard();
setupProviderEditor();
setupActivityList();
initThemeManager();
setupTabManager();
setupReasoningModal();

async function initialize(): Promise<void> {
  const [configResult, proxyResult, ideResult, appResult, cliResult, activityResult] = await Promise.allSettled([
    configService.getConfig(),
    proxyService.getStatus(),
    hostService.discoverIde(),
    hostService.discoverApp(),
    hostService.discoverCli(),
    activityService.getLog(),
  ]);

  const failures: string[] = [];
  if (configResult.status === "fulfilled") {
    store.setConfig(configResult.value);
    renderProviders();
  } else {
    failures.push("上游服务配置");
    const providerList = document.querySelector<HTMLDivElement>("#provider-list");
    if (providerList) {
      providerList.replaceChildren();
      const error = document.createElement("p");
      error.className = "empty-state error-state";
      error.textContent = `配置读取失败：${errorMessage(configResult.reason)}`;
      providerList.append(error);
    }
  }

  if (proxyResult.status === "fulfilled") {
    renderProxy(proxyResult.value);
  } else {
    store.setProxyStatusFailed();
    failures.push("代理状态");
    const proxyState = document.querySelector("#proxy-state");
    if (proxyState) proxyState.textContent = "读取失败";
  }

  if (ideResult.status === "fulfilled") {
    renderIde(ideResult.value);
  } else {
    store.setIdeStatusFailed();
    failures.push("IDE 状态");
    const ideState = document.querySelector("#ide-state");
    if (ideState) ideState.textContent = "读取失败";
  }

  if (appResult.status === "fulfilled") {
    renderApp(appResult.value);
  } else {
    store.setAppStatusFailed();
    failures.push("App 状态");
  }

  if (cliResult.status === "fulfilled") {
    renderCli(cliResult.value);
  } else {
    store.setCliStatusFailed();
    failures.push("CLI 状态");
  }

  if (activityResult.status === "fulfilled") {
    setActivityItems(activityResult.value);
  } else {
    failures.push("调用日志");
  }

  renderReadiness();
  if (failures.length > 0) {
    showNotice(`部分状态读取失败：${failures.join("、")}`, "error");
  }
}

void initialize();
