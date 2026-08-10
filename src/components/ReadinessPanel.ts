import { store } from "../store/appStore";
import { element, withBusy } from "../utils/domUtils";
import { launchIde, launchApp, refreshHostStatuses } from "../controllers/hostController";
import { confirmHostAction } from "./ConfirmModal";
import { showNotice } from "./NoticeBar";
import { t } from "../i18n";

function getRunningClients(): { ide: boolean; app: boolean; labels: string[] } {
  const ideRunning = store.ideStatus?.ideRunning ?? false;
  const appRunning = store.appStatus?.appRunning ?? false;
  const labels: string[] = [];
  if (ideRunning) labels.push(t("overview.clientIde"));
  if (appRunning) labels.push(t("overview.clientApp"));
  return { ide: ideRunning, app: appRunning, labels };
}

export function renderReadinessPanel(): void {
  const badge = element<HTMLSpanElement>("#readiness-badge");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  const actions = element<HTMLDivElement>("#readiness-actions");

  const { labels } = getRunningClients();
  if (labels.length > 0) {
    badge.hidden = false;
    badge.textContent = t("overview.restartNeededBadge");
    detail.textContent = t("overview.restartNeededDetail", { clients: labels.join("、") });
    actions.hidden = false;
  } else {
    badge.hidden = true;
    detail.textContent = t("overview.configurationRestartNotice");
    actions.hidden = true;
  }
}

export function setupReadinessPanel(): void {
  const restartButton = element<HTMLButtonElement>("#bulk-restart-btn");
  restartButton.addEventListener("click", () => {
    void (async () => {
      const { ide, app, labels } = getRunningClients();
      if (labels.length === 0) return;

      const confirmed = await confirmHostAction(
        t("overview.bulkRestartConfirm", { clients: labels.join("、") }),
        t("overview.bulkRestartTitle"),
        t("overview.restart"),
        t("overview.hostCancel"),
      );
      if (!confirmed) return;

      await withBusy(restartButton, async () => {
        const tasks: Promise<void>[] = [];
        if (ide) tasks.push(launchIde());
        if (app) tasks.push(launchApp());
        await Promise.all(tasks);
        showNotice(t("overview.bulkRestartSuccess"));
        window.setTimeout(() => void refreshHostStatuses().catch(() => undefined), 800);
      }, showNotice, t("overview.bulkRestarting"));
    })();
  });
}
