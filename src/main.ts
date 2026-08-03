import "./styles.css";
import { store } from "./store/appStore";
import { configService } from "./services/configService";
import { proxyService } from "./services/proxyService";

import { activityService } from "./services/activityService";

import { setupProxyCard, renderProxy, renderProxyLoadFailure } from "./components/ProxyCard";
import { setupIdeCard, renderIde, renderIdeLoadFailure } from "./components/IdeCard";
import { setupAppCard, renderApp, renderAppLoadFailure } from "./components/AppCard";
import { setupCliCard, renderCli, renderCliLoadFailure } from "./components/CliCard";
import { renderReadiness, setupReadinessPanel } from "./components/ReadinessPanel";
import { renderProviders } from "./components/ProviderList";
import { setupProviderEditor } from "./components/ProviderEditor";
import { setActivityItems, setActivityLoadFailed, setupActivityList } from "./components/ActivityList";
import { showNotice, setupNoticeBar } from "./components/NoticeBar";
import { initThemeManager } from "./components/ThemeManager";
import { setupTabManager } from "./components/TabManager";
import { setupSettingsView } from "./components/SettingsView";
import { setupUpdateManager } from "./components/UpdateManager";
import { errorMessage } from "./utils/domUtils";
import { setupReasoningModal } from "./components/ReasoningModal";
import { refreshApp, refreshCli, refreshHostStatuses, refreshIde } from "./controllers/hostController";

import { updateDOMTranslations, subscribeLanguage, t } from "./i18n";

setupNoticeBar();
setupProxyCard();
setupIdeCard();
setupAppCard();
setupCliCard();
setupProviderEditor();
setupActivityList();
initThemeManager();
setupTabManager();
setupSettingsView();
setupReasoningModal();
setupReadinessPanel();

updateDOMTranslations();
setupUpdateManager();

function renderRuntimeState(): void {
  if (store.proxyStatusLoadFailed) renderProxyLoadFailure(t("overview.loadFailed"));
  else if (store.proxyStatus) renderProxy(store.proxyStatus);
  if (store.ideStatusLoadFailed) renderIdeLoadFailure(t("overview.loadFailed"));
  else if (store.ideStatus) renderIde(store.ideStatus);
  if (store.appStatusLoadFailed) renderAppLoadFailure(t("overview.loadFailed"));
  else if (store.appStatus) renderApp(store.appStatus);
  if (store.cliStatusLoadFailed) renderCliLoadFailure(t("overview.loadFailed"));
  else if (store.cliStatus) renderCli(store.cliStatus);
  renderReadiness();
}

store.subscribe(renderRuntimeState);
store.subscribeConfig(renderProviders);
renderProviders();

subscribeLanguage(() => {
  renderProviders();
  renderRuntimeState();
});

async function initialize(): Promise<void> {
  const [configResult, proxyResult, ideResult, appResult, cliResult, activityResult] = await Promise.allSettled([
    configService.getConfig(),
    proxyService.getStatus(),
    refreshIde(),
    refreshApp(),
    refreshCli(),
    activityService.getLog(),
  ]);

  const failures: string[] = [];
  if (configResult.status === "fulfilled") {
    store.setConfig(configResult.value);
    const portInput = document.querySelector<HTMLInputElement>("#settings-proxy-port");
    if (portInput) portInput.value = String(configResult.value.proxy_port);
  } else {
    failures.push(t("models.title"));
    store.setConfigFailed(errorMessage(configResult.reason));
  }

  if (proxyResult.status === "fulfilled") {
    store.setProxyStatus(proxyResult.value);
  } else {
    store.setProxyStatusFailed();
    failures.push(t("overview.proxyServer"));
    const proxyState = document.querySelector<HTMLSpanElement>("#proxy-state");
    if (proxyState) {
      proxyState.textContent = t("overview.loadFailed");
      proxyState.className = "status-pill error";
    }
  }

  if (ideResult.status === "rejected") {
    failures.push(t("overview.ideStatusItem"));
    renderIdeLoadFailure(errorMessage(ideResult.reason));
  }

  if (appResult.status === "rejected") {
    failures.push(t("overview.appStatusItem"));
    renderAppLoadFailure(errorMessage(appResult.reason));
  }

  if (cliResult.status === "rejected") {
    failures.push(t("overview.cliStatusItem"));
    renderCliLoadFailure(errorMessage(cliResult.reason));
  }

  if (activityResult.status === "fulfilled") {
    setActivityItems(activityResult.value);
  } else {
    failures.push(t("overview.activityStatusItem"));
    setActivityLoadFailed(errorMessage(activityResult.reason));
  }

  renderReadiness();
  if (failures.length > 0) {
    showNotice(t("overview.statusLoadFailed", { items: failures.join(", ") }), "error");
  }
}

void initialize();

window.addEventListener("focus", () => {
  void refreshHostStatuses();
});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void refreshHostStatuses();
});
