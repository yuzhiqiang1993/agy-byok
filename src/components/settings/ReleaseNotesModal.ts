import { openExternalUrl as openExternalUrlCommand } from "../../controllers/hostController";
import { t } from "../../i18n";
import type { Update } from "../../services/updateService";
import { element } from "../../utils/domUtils";
import { formatMarkdownReleaseNotes } from "./UpdateView";

let currentUpdate: Update | null = null;
let currentInstallHandler: (() => void) | null = null;

function handleKeyDown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    closeReleaseNotesModal();
  }
}

export function openReleaseNotesModal(
  update: Update,
  currentVersion: string,
  onInstall: () => void,
): void {
  currentUpdate = update;
  currentInstallHandler = onInstall;

  const modal = element<HTMLElement>("#release-notes-modal");
  const versionPill = element<HTMLElement>("#release-notes-modal-version");
  const subtitle = element<HTMLElement>("#release-notes-modal-subtitle");
  const content = element<HTMLElement>("#release-notes-modal-content");

  versionPill.textContent = t("settings.versionTag", { version: update.version });
  subtitle.textContent = t("settings.releaseVersionSubtitle", {
    version: update.version,
    current: currentVersion || "—",
  });
  content.innerHTML = formatMarkdownReleaseNotes(update.body ?? "");

  modal.hidden = false;
  document.body.classList.add("modal-open");
  window.addEventListener("keydown", handleKeyDown);
}

export function closeReleaseNotesModal(): void {
  const modal = element<HTMLElement>("#release-notes-modal");
  modal.hidden = true;
  document.body.classList.remove("modal-open");
  window.removeEventListener("keydown", handleKeyDown);
  currentUpdate = null;
  currentInstallHandler = null;
}

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

export function setupReleaseNotesModal(): void {
  const closeBtn = document.querySelector("#close-release-notes-modal");
  const cancelBtn = document.querySelector("#cancel-release-notes-modal");
  const backdrop = document.querySelector("#release-notes-modal-backdrop");
  const installBtn = document.querySelector("#install-update-from-modal");
  const githubLink = document.querySelector("#release-notes-github-link");

  closeBtn?.addEventListener("click", closeReleaseNotesModal);
  cancelBtn?.addEventListener("click", closeReleaseNotesModal);
  backdrop?.addEventListener("click", closeReleaseNotesModal);

  githubLink?.addEventListener("click", () => void openGithubRelease());
  installBtn?.addEventListener("click", () => {
    const handler = currentInstallHandler;
    closeReleaseNotesModal();
    if (handler) handler();
  });
}
