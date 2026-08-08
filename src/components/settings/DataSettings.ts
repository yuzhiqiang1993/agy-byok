import { clearActivityLog } from "../../controllers/activityController";
import { getConfigPath, openConfigDir } from "../../controllers/hostController";
import { t } from "../../i18n";
import { errorMessage } from "../../utils/errorUtils";
import { showNotice } from "../NoticeBar";

export async function openSettingsConfigDirectory(): Promise<void> {
  try {
    await openConfigDir();
    showNotice(t("settings.configDirOpened"), "success");
  } catch (error) {
    showNotice(t("settings.configDirOpenFailed", { message: errorMessage(error) }), "error");
  }
}

export function setupDataSettings(): void {
  const configPath = document.querySelector<HTMLElement>("#settings-config-path-text");
  if (configPath) {
    void getConfigPath()
      .then((path) => { configPath.textContent = path; })
      .catch(() => { configPath.textContent = "—"; });
  }
  document.querySelector("#open-config-dir")?.addEventListener(
    "click",
    () => void openSettingsConfigDirectory(),
  );
  document.querySelector("#settings-clear-logs-btn")?.addEventListener("click", () => {
    void clearActivityLog()
      .then(() => showNotice(t("activity.clearSuccess")))
      .catch((error: unknown) => {
        showNotice(t("settings.clearLogsFailed", { message: errorMessage(error) }), "error");
      });
  });
}
