import { t } from "../../i18n";
import type { Update } from "../../services/updateService";
import { element } from "../../utils/domUtils";

type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "ready" | "error";

export interface UpdateManagerState {
  phase: UpdatePhase;
  currentVersion: string;
  pendingUpdate: Update | null;
  hasChecked: boolean;
  downloadedBytes: number;
  contentLength: number | undefined;
}

export interface UpdateView {
  versionTag: HTMLSpanElement;
  status: HTMLSpanElement;
  notes: HTMLParagraphElement;
  checkButton: HTMLButtonElement;
  installButton: HTMLButtonElement;
  progress: HTMLProgressElement;
}

export function createUpdateView(): UpdateView {
  return {
    versionTag: element<HTMLSpanElement>("#app-version"),
    status: element<HTMLSpanElement>("#update-status"),
    notes: element<HTMLParagraphElement>("#update-notes"),
    checkButton: element<HTMLButtonElement>("#check-for-updates"),
    installButton: element<HTMLButtonElement>("#install-update"),
    progress: element<HTMLProgressElement>("#update-progress"),
  };
}

function updateStatusText(state: UpdateManagerState): string {
  if (state.phase === "checking") return t("settings.checkingUpdates");
  if (state.phase === "available") {
    return state.pendingUpdate
      ? t("settings.updateAvailable", { version: state.pendingUpdate.version })
      : t("settings.updateIdle");
  }
  if (state.phase === "downloading") return t("settings.downloadingUpdate");
  if (state.phase === "ready") return t("settings.updateReady");
  if (state.phase === "error") return t("settings.updateCheckFailedShort");
  return state.hasChecked ? t("settings.latestVersion") : t("settings.updateIdle");
}

export function renderUpdateView(view: UpdateView, state: UpdateManagerState): void {
  view.versionTag.textContent = t("settings.versionTag", {
    version: state.currentVersion || "—",
  });
  view.status.textContent = updateStatusText(state);
  const operationInProgress = state.phase === "checking" || state.phase === "downloading";
  view.checkButton.disabled = operationInProgress;
  view.checkButton.textContent = state.phase === "checking"
    ? t("settings.checkingUpdates")
    : t("settings.checkUpdates");
  view.installButton.hidden = state.phase !== "available";
  view.installButton.disabled = state.phase !== "available";
  view.notes.hidden = state.phase !== "available" || !state.pendingUpdate?.body;
  view.notes.textContent = state.pendingUpdate?.body ?? "";
  view.progress.hidden = state.phase !== "downloading";
  if (state.phase === "downloading" && state.contentLength) {
    view.progress.max = state.contentLength;
    view.progress.value = state.downloadedBytes;
  } else {
    view.progress.removeAttribute("value");
  }
}
