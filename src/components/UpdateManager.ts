import { subscribeLanguage, t } from "../i18n";
import {
  checkForUpdate,
  downloadAndInstallUpdate,
  getApplicationVersion,
  isTauriRuntime,
  relaunchApplication,
  type DownloadEvent,
  type Update,
} from "../services/updateService";
import { errorMessage } from "../utils/errorUtils";
import { showNotice } from "./NoticeBar";
import {
  createUpdateView,
  renderUpdateView,
  type UpdateManagerState,
  type UpdateView,
} from "./settings/UpdateView";

async function clearPendingUpdate(state: UpdateManagerState): Promise<void> {
  const update = state.pendingUpdate;
  state.pendingUpdate = null;
  if (!update) return;
  try {
    await update.close();
  } catch (error) {
    console.error("Unable to release updater resource", error);
  }
}

function handleDownloadEvent(
  state: UpdateManagerState,
  view: UpdateView,
  event: DownloadEvent,
): void {
  if (event.event === "Started") {
    state.contentLength = event.data.contentLength;
    state.downloadedBytes = 0;
  } else if (event.event === "Progress") {
    state.downloadedBytes += event.data.chunkLength;
  } else if (state.contentLength) {
    state.downloadedBytes = state.contentLength;
  }
  renderUpdateView(view, state);
}

async function loadVersion(state: UpdateManagerState, view: UpdateView): Promise<void> {
  try {
    state.currentVersion = await getApplicationVersion();
    renderUpdateView(view, state);
  } catch {
    // 版本元数据读取失败不影响用户手动检查更新。
  }
}

async function checkUpdate(
  state: UpdateManagerState,
  view: UpdateView,
  manual: boolean,
): Promise<void> {
  if (state.phase === "checking" || state.phase === "downloading") return;
  state.phase = "checking";
  state.hasChecked = false;
  renderUpdateView(view, state);
  await clearPendingUpdate(state);

  try {
    state.pendingUpdate = await checkForUpdate();
    state.hasChecked = true;
    state.phase = state.pendingUpdate ? "available" : "idle";
    renderUpdateView(view, state);
    if (state.pendingUpdate) {
      showNotice(t("settings.updateAvailable", { version: state.pendingUpdate.version }));
    } else if (manual) {
      showNotice(t("settings.latestVersion"));
    }
  } catch (error) {
    state.hasChecked = true;
    state.phase = "error";
    renderUpdateView(view, state);
    if (manual) {
      showNotice(t("settings.updateCheckFailed", { message: errorMessage(error) }), "error");
    }
  }
}

async function refreshPendingUpdate(
  state: UpdateManagerState,
  view: UpdateView,
): Promise<Update | null | undefined> {
  state.phase = "checking";
  state.hasChecked = false;
  renderUpdateView(view, state);
  await clearPendingUpdate(state);
  try {
    const update = await checkForUpdate();
    state.hasChecked = true;
    return update;
  } catch (error) {
    state.hasChecked = true;
    state.phase = "error";
    renderUpdateView(view, state);
    showNotice(t("settings.updateCheckFailed", { message: errorMessage(error) }), "error");
    return undefined;
  }
}

async function installUpdate(state: UpdateManagerState, view: UpdateView): Promise<void> {
  if (!state.pendingUpdate || state.phase !== "available") return;
  const update = await refreshPendingUpdate(state, view);
  if (update === undefined) return;
  if (update === null) {
    state.phase = "idle";
    renderUpdateView(view, state);
    showNotice(t("settings.latestVersion"));
    return;
  }

  state.pendingUpdate = update;
  state.phase = "downloading";
  state.downloadedBytes = 0;
  state.contentLength = undefined;
  renderUpdateView(view, state);
  try {
    await downloadAndInstallUpdate(update, (event) => handleDownloadEvent(state, view, event));
  } catch (error) {
    state.phase = "available";
    renderUpdateView(view, state);
    showNotice(t("settings.updateInstallFailed", { message: errorMessage(error) }), "error");
    return;
  }

  await clearPendingUpdate(state);
  state.phase = "ready";
  renderUpdateView(view, state);
  showNotice(t("settings.updateRestarting"));
  try {
    await relaunchApplication();
  } catch (error) {
    showNotice(t("settings.updateRestartFailed", { message: errorMessage(error) }), "error");
  }
}

export function setupUpdateManager(): void {
  if (!isTauriRuntime()) return;
  const view = createUpdateView();
  const state: UpdateManagerState = {
    phase: "idle",
    currentVersion: "",
    pendingUpdate: null,
    hasChecked: false,
    downloadedBytes: 0,
    contentLength: undefined,
  };
  view.checkButton.addEventListener("click", () => void checkUpdate(state, view, true));
  view.installButton.addEventListener("click", () => void installUpdate(state, view));
  subscribeLanguage(() => renderUpdateView(view, state));
  renderUpdateView(view, state);
  void loadVersion(state, view);
  window.setTimeout(() => void checkUpdate(state, view, false), 3500);
}
