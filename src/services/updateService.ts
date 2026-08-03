import { getVersion } from "@tauri-apps/api/app";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

const CHECK_TIMEOUT_MS = 15_000;
const DOWNLOAD_TIMEOUT_MS = 300_000;

export type { DownloadEvent, Update };

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getApplicationVersion(): Promise<string> {
  return getVersion();
}

export async function checkForUpdate(): Promise<Update | null> {
  return check({ timeout: CHECK_TIMEOUT_MS });
}

export async function downloadAndInstallUpdate(
  update: Update,
  onEvent: (event: DownloadEvent) => void,
): Promise<void> {
  await update.downloadAndInstall(onEvent, { timeout: DOWNLOAD_TIMEOUT_MS });
}

export async function relaunchApplication(): Promise<void> {
  await relaunch();
}
