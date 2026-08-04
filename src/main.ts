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
import { isTauriRuntime } from "./services/updateService";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { errorMessage } from "./utils/domUtils";
import { setupReasoningModal } from "./components/ReasoningModal";
import { refreshApp, refreshCli, refreshHostStatuses, refreshIde } from "./controllers/hostController";

import { updateDOMTranslations, subscribeLanguage, t } from "./i18n";

const OVERLAY_TITLEBAR_HEIGHT = 40;
const WINDOW_INTERACTIVE_SELECTOR = "button, input, select, textarea, a, [data-tauri-drag-region=\"false\"], .no-drag";
const EDITABLE_SELECTOR = "input, textarea, select, [contenteditable=\"true\"]";

function runWindowAction(action: Promise<void>, message: string): void {
  void action.catch((error: unknown) => {
    console.error(message, error);
  });
}

async function toggleFullscreen(appWindow: ReturnType<typeof getCurrentWindow>): Promise<void> {
  const fullscreen = await appWindow.isFullscreen();
  await appWindow.setFullscreen(!fullscreen);
}

function setupWindowDragging(): void {
  if (!isTauriRuntime()) return;

  const appWindow = getCurrentWindow();
  document.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;

    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest(WINDOW_INTERACTIVE_SELECTOR)) return;

    const isMarkedDragRegion = target.closest("[data-tauri-drag-region]") !== null;
    const isOverlayTitlebarRegion = event.clientY <= OVERLAY_TITLEBAR_HEIGHT;
    if (!isMarkedDragRegion && !isOverlayTitlebarRegion) return;

    event.preventDefault();
    const windowAction = event.detail === 2
      ? appWindow.toggleMaximize()
      : appWindow.startDragging();
    runWindowAction(windowAction, "Unable to update the window state");
  });
}

function setupWindowShortcuts(): void {
  if (!isTauriRuntime()) return;

  const appWindow = getCurrentWindow();
  document.addEventListener("keydown", (event) => {
    if (event.defaultPrevented || event.repeat) return;

    const target = event.target;
    if (target instanceof Element && target.closest(EDITABLE_SELECTOR)) return;

    const key = event.key.toLowerCase();
    const primaryModifier = event.metaKey || event.ctrlKey;

    if (primaryModifier && key === "w" && !event.altKey && !event.shiftKey) {
      event.preventDefault();
      runWindowAction(appWindow.close(), "Unable to close the window");
      return;
    }

    if (primaryModifier && key === "m" && !event.altKey && !event.shiftKey) {
      event.preventDefault();
      runWindowAction(appWindow.minimize(), "Unable to minimize the window");
      return;
    }

    const fullscreenShortcut =
      event.key === "F11" ||
      (event.metaKey && event.ctrlKey && key === "f") ||
      (primaryModifier && event.shiftKey && key === "f");
    if (fullscreenShortcut) {
      event.preventDefault();
      runWindowAction(toggleFullscreen(appWindow), "Unable to toggle fullscreen");
      return;
    }

    if (event.key === "Escape") {
      runWindowAction(appWindow.setFullscreen(false), "Unable to exit fullscreen");
    }
  });
}

setupWindowDragging();
setupWindowShortcuts();
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
