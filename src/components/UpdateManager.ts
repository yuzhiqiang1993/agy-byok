import {
  checkForUpdate,
  downloadAndInstallUpdate,
  getApplicationVersion,
  isTauriRuntime,
  relaunchApplication,
  type DownloadEvent,
  type Update,
} from "../services/updateService";
import { errorMessage, element } from "../utils/domUtils";
import { subscribeLanguage, t } from "../i18n";
import { showNotice } from "./NoticeBar";

type UpdateState = "idle" | "checking" | "available" | "downloading" | "ready" | "error";

export function setupUpdateManager(): void {
  if (!isTauriRuntime()) return;

  const versionTag = element<HTMLSpanElement>("#app-version");
  const status = element<HTMLSpanElement>("#update-status");
  const notes = element<HTMLParagraphElement>("#update-notes");
  const checkButton = element<HTMLButtonElement>("#check-for-updates");
  const installButton = element<HTMLButtonElement>("#install-update");
  const progress = element<HTMLProgressElement>("#update-progress");

  let state: UpdateState = "idle";
  let currentVersion = "";
  let pendingUpdate: Update | null = null;
  let hasChecked = false;
  let downloadedBytes = 0;
  let contentLength: number | undefined;

  function render(): void {
    versionTag.textContent = currentVersion ? `v${currentVersion}` : "v—";

    switch (state) {
      case "checking":
        status.textContent = t("settings.checkingUpdates");
        break;
      case "available":
        status.textContent = pendingUpdate
          ? t("settings.updateAvailable", { version: pendingUpdate.version })
          : t("settings.updateIdle");
        break;
      case "downloading":
        status.textContent = t("settings.downloadingUpdate");
        break;
      case "ready":
        status.textContent = t("settings.updateReady");
        break;
      case "error":
        status.textContent = t("settings.updateCheckFailedShort");
        break;
      case "idle":
        status.textContent = hasChecked
          ? t("settings.latestVersion")
          : t("settings.updateIdle");
        break;
    }

    checkButton.disabled = state === "checking" || state === "downloading";
    checkButton.textContent = state === "checking"
      ? t("settings.checkingUpdates")
      : t("settings.checkUpdates");

    installButton.hidden = state !== "available";
    installButton.disabled = state !== "available";

    notes.hidden = state !== "available" || !pendingUpdate?.body;
    notes.textContent = pendingUpdate?.body ?? "";

    progress.hidden = state !== "downloading";
    if (state === "downloading" && contentLength) {
      progress.max = contentLength;
      progress.value = downloadedBytes;
    } else {
      progress.removeAttribute("value");
    }
  }

  function handleDownloadEvent(event: DownloadEvent): void {
    switch (event.event) {
      case "Started":
        contentLength = event.data.contentLength;
        downloadedBytes = 0;
        break;
      case "Progress":
        downloadedBytes += event.data.chunkLength;
        break;
      case "Finished":
        if (contentLength) downloadedBytes = contentLength;
        break;
    }
    render();
  }

  async function loadVersion(): Promise<void> {
    try {
      currentVersion = await getApplicationVersion();
      render();
    } catch {
      // The update controls remain usable even if the version metadata cannot be read.
    }
  }

  async function checkUpdate(manual: boolean): Promise<void> {
    if (state === "checking" || state === "downloading") return;

    state = "checking";
    pendingUpdate = null;
    notes.textContent = "";
    hasChecked = false;
    render();

    try {
      pendingUpdate = await checkForUpdate();
      hasChecked = true;
      state = pendingUpdate ? "available" : "idle";
      render();

      if (pendingUpdate) {
        showNotice(t("settings.updateAvailable", { version: pendingUpdate.version }));
      } else if (manual) {
        showNotice(t("settings.latestVersion"));
      }
    } catch (error) {
      hasChecked = true;
      state = "error";
      render();
      if (manual) {
        showNotice(t("settings.updateCheckFailed", { message: errorMessage(error) }), "error");
      }
    }
  }

  async function installUpdate(): Promise<void> {
    if (!pendingUpdate || state !== "available") return;

    state = "downloading";
    downloadedBytes = 0;
    contentLength = undefined;
    render();

    try {
      await downloadAndInstallUpdate(pendingUpdate, handleDownloadEvent);
      state = "ready";
      render();
      showNotice(t("settings.updateRestarting"));
      await relaunchApplication();
    } catch (error) {
      state = "available";
      render();
      showNotice(t("settings.updateInstallFailed", { message: errorMessage(error) }), "error");
    }
  }

  checkButton.addEventListener("click", () => void checkUpdate(true));
  installButton.addEventListener("click", () => void installUpdate());
  subscribeLanguage(() => render());

  void loadVersion();
  window.setTimeout(() => void checkUpdate(false), 3500);

}
