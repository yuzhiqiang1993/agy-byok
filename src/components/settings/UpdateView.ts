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
  checkButton: HTMLButtonElement;
  installButton: HTMLButtonElement;
  restartButton: HTMLButtonElement;
  viewReleaseButton: HTMLButtonElement;
  progressContainer: HTMLDivElement;
  progressDetail: HTMLSpanElement;
  progress: HTMLProgressElement;
  settingsNavBadge: HTMLSpanElement;
  aboutNavBadge: HTMLSpanElement;
}

export function createUpdateView(): UpdateView {
  return {
    versionTag: element<HTMLSpanElement>("#app-version"),
    status: element<HTMLSpanElement>("#update-status"),
    checkButton: element<HTMLButtonElement>("#check-for-updates"),
    installButton: element<HTMLButtonElement>("#install-update"),
    restartButton: element<HTMLButtonElement>("#restart-app-now"),
    viewReleaseButton: element<HTMLButtonElement>("#view-release-notes"),
    progressContainer: element<HTMLDivElement>("#update-progress-container"),
    progressDetail: element<HTMLSpanElement>("#update-progress-detail"),
    progress: element<HTMLProgressElement>("#update-progress"),
    settingsNavBadge: element<HTMLSpanElement>("#settings-nav-badge"),
    aboutNavBadge: element<HTMLSpanElement>("#about-nav-badge"),
  };
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

export function formatMarkdownReleaseNotes(raw: string): string {
  if (!raw.trim()) return "";
  const lines = raw.split("\n");
  const htmlParts: string[] = [];
  let inList = false;

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (inList) {
        htmlParts.push("</ul>");
        inList = false;
      }
      continue;
    }

    if (trimmed.startsWith("### ")) {
      if (inList) {
        htmlParts.push("</ul>");
        inList = false;
      }
      htmlParts.push(`<h4>${escapeHtml(trimmed.slice(4))}</h4>`);
      continue;
    }

    if (trimmed.startsWith("## ")) {
      if (inList) {
        htmlParts.push("</ul>");
        inList = false;
      }
      htmlParts.push(`<h3>${escapeHtml(trimmed.slice(3))}</h3>`);
      continue;
    }

    if (trimmed.startsWith("# ")) {
      if (inList) {
        htmlParts.push("</ul>");
        inList = false;
      }
      htmlParts.push(`<h3>${escapeHtml(trimmed.slice(2))}</h3>`);
      continue;
    }

    if (trimmed.startsWith("- ") || trimmed.startsWith("* ")) {
      if (!inList) {
        htmlParts.push("<ul class=\"update-notes-list\">");
        inList = true;
      }
      let content = escapeHtml(trimmed.slice(2));
      content = content.replace(/`([^`]+)`/g, "<code>$1</code>");
      content = content.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
      htmlParts.push(`<li>${content}</li>`);
      continue;
    }

    if (inList) {
      htmlParts.push("</ul>");
      inList = false;
    }
    let content = escapeHtml(trimmed);
    content = content.replace(/`([^`]+)`/g, "<code>$1</code>");
    content = content.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    htmlParts.push(`<p>${content}</p>`);
  }

  if (inList) {
    htmlParts.push("</ul>");
  }

  return htmlParts.join("");
}

function updateStatusText(state: UpdateManagerState): string {
  if (state.phase === "checking") return t("settings.checkingUpdates");
  if (state.phase === "available") {
    return state.pendingUpdate
      ? t("settings.updateAvailableWithVersion", {
          latest: state.pendingUpdate.version,
          current: state.currentVersion || "—",
        })
      : t("settings.updateIdle");
  }
  if (state.phase === "downloading") return t("settings.downloadingUpdate");
  if (state.phase === "ready") return t("settings.updateReady");
  if (state.phase === "error") return t("settings.updateCheckFailedShort");
  return state.hasChecked
    ? t("settings.latestVersionWithCurrent", { version: state.currentVersion || "—" })
    : t("settings.updateIdle");
}

export function renderUpdateView(view: UpdateView, state: UpdateManagerState): void {
  view.versionTag.textContent = t("settings.versionTag", {
    version: state.currentVersion || "—",
  });
  view.status.textContent = updateStatusText(state);

  // Badges on nav items
  const hasAvailableUpdate = state.phase === "available" || state.phase === "ready";
  view.settingsNavBadge.hidden = !hasAvailableUpdate;
  view.aboutNavBadge.hidden = !hasAvailableUpdate;

  // Single-action button logic: Only one primary/secondary button is shown at a time
  const isChecking = state.phase === "checking";
  const isAvailable = state.phase === "available";
  const isReady = state.phase === "ready";
  const isDownloading = state.phase === "downloading";

  // Check button: visible only in idle / checking / error
  view.checkButton.hidden = isAvailable || isReady || isDownloading;
  view.checkButton.disabled = isChecking;
  view.checkButton.textContent = isChecking
    ? t("settings.checkingUpdates")
    : t("settings.checkUpdates");

  // Install button: visible only in available
  view.installButton.hidden = !isAvailable;
  view.installButton.disabled = !isAvailable;

  // Restart button: visible only in ready
  view.restartButton.hidden = !isReady;
  view.restartButton.disabled = !isReady;

  // Inline "View release notes" link: visible only when an update is available and notes body exists
  view.viewReleaseButton.hidden = !isAvailable || !state.pendingUpdate?.body;

  // Progress Section
  view.progressContainer.hidden = !isDownloading;
  if (isDownloading) {
    if (state.contentLength && state.contentLength > 0) {
      view.progress.max = state.contentLength;
      view.progress.value = state.downloadedBytes;
      const percent = Math.min(100, Math.round((state.downloadedBytes / state.contentLength) * 100));
      view.progressDetail.textContent = t("settings.updateDownloadProgress", {
        percent: String(percent),
        downloaded: formatBytes(state.downloadedBytes),
        total: formatBytes(state.contentLength),
      });
    } else {
      view.progress.removeAttribute("value");
      view.progressDetail.textContent = t("settings.updateDownloadProgressIndeterminate", {
        downloaded: formatBytes(state.downloadedBytes),
      });
    }
  } else {
    view.progressDetail.textContent = "";
  }
}
