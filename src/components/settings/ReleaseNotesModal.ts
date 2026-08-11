import { openExternalUrl as openExternalUrlCommand } from "../../controllers/hostController";
import { t } from "../../i18n";
import type { Update } from "../../services/updateService";
import { createModal } from "../common/Modal";
import { formatMarkdownReleaseNotes } from "./UpdateView";

let currentUpdate: Update | null = null;

async function openGithubRelease(): Promise<void> {
  const version = currentUpdate?.version;
  const url = version
    ? `https://github.com/yuzhiqiang1993/agy-byok/releases/tag/app-v${version}`
    : "https://github.com/yuzhiqiang1993/agy-byok/releases/latest";
  try {
    await openExternalUrlCommand(url);
  } catch {
    window.open(url, "_blank");
  }
}

export function openReleaseNotesModal(
  update: Update,
  currentVersion: string,
  onInstall: () => void,
): void {
  currentUpdate = update;

  const body = document.createElement("div");
  body.className = "release-notes-modal-content";
  body.innerHTML = formatMarkdownReleaseNotes(update.body ?? "");

  const titleExtras = [];
  const versionBadge = document.createElement("span");
  versionBadge.className = "update-version-badge";
  versionBadge.textContent = t("settings.versionTag", { version: update.version });
  titleExtras.push(versionBadge);

  const githubLink = document.createElement("button");
  githubLink.type = "button";
  githubLink.className = "secondary compact-button";
  githubLink.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 22v-4a4.8 4.8 0 0 0-1-3.24c3-.34 6-1.5 6-6.76 0-1.5-.5-2.8-1.5-3.8.15-.38.65-1.8-.15-3.8 0 0-1.25-.4-3.85 1.35a11 11 0 0 0-7 0C4.25 2.1 3 2.5 3 2.5c-.8 2-.3 3.4-.15 3.8-1 1-1.5 2.3-1.5 3.8 0 5.2 3 6.4 6 6.76a4.8 4.8 0 0 0-1 3.24v4"/><path d="M9 18c-4.51 2-5-2-7-2"/></svg> ${t("settings.viewOnGithub")}`;
  githubLink.addEventListener("click", () => void openGithubRelease());
  titleExtras.push(githubLink);

  const modal = createModal({
    title: t("settings.releaseNotesTitle"),
    subtitle: t("settings.releaseVersionSubtitle", {
      version: update.version,
      current: currentVersion || "—",
    }),
    titleExtras,
    body,
    dialogClassName: "release-notes-modal-dialog",
    okLabel: t("settings.installUpdate"),
    cancelLabel: t("modal.close"),
    onOk: () => {
      modal.close();
      onInstall();
    },
    onCancel: () => {
      // Just close
    }
  });

  const originalClose = modal.close;
  modal.close = () => {
    currentUpdate = null;
    originalClose();
  };
}
