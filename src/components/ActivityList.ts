import type { ActivityItem } from "../types/activity";
import {
  activityState,
  nextActivityRequestVersion,
  setActivityItems as setActivityItemsState,
  setActivityLoadFailed as setActivityLoadFailedState,
} from "../features/activity/activityState";
import {
  getActivityLog,
  clearActivityLog as clearActivityLogCommand,
  subscribeActivityCleared,
} from "../controllers/activityController";
import {
  element,
  armDestructiveButton,
  setButtonUnavailable,
  withBusy,
} from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import { isActivityFailure } from "../features/activity/activityPresentation";
import { t, subscribeLanguage } from "../i18n";
import { renderActivityItem } from "./ActivityItem";
import { showNotice } from "./NoticeBar";

function renderActivityLog(): void {
  const activityCount = element<HTMLSpanElement>("#activity-count");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const activityList = element<HTMLDivElement>("#activity-list");

  if (activityState.loadError) {
    activityCount.textContent = t("overview.loadFailed");
    activityCount.setAttribute("aria-label", activityCount.textContent);
    setButtonUnavailable(clearActivityButton, true);
    activityList.replaceChildren();
    const error = document.createElement("p");
    error.className = "empty-state error-state";
    error.textContent = t("activity.logLoadFailed", { message: activityState.loadError });
    activityList.append(error);
    return;
  }

  const failures = activityState.items.filter(isActivityFailure).length;
  const visibleItems = activityState.failedOnly
    ? activityState.items.filter(isActivityFailure)
    : activityState.items;
  activityCount.textContent = activityState.failedOnly
    ? t("activity.countBadgeFiltered", {
        failed: visibleItems.length,
        total: activityState.items.length,
      })
    : t("activity.countBadge", { total: activityState.items.length, failed: failures });
  activityCount.setAttribute("aria-label", activityCount.textContent);
  setButtonUnavailable(clearActivityButton, activityState.items.length === 0);

  const oldScrollTop = activityList.scrollTop;
  const oldScrollHeight = activityList.scrollHeight;
  const nearTop = oldScrollTop < 24;
  activityList.replaceChildren();

  if (visibleItems.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = activityState.items.length === 0
      ? t("activity.emptyDesc")
      : t("activity.emptyDescFiltered");
    activityList.append(empty);
    return;
  }

  activityList.append(...visibleItems.map(renderActivityItem));
  activityList.scrollTop = nearTop
    ? 0
    : oldScrollTop + (activityList.scrollHeight - oldScrollHeight);
}

export function setActivityItems(items: ActivityItem[]): void {
  setActivityItemsState(items);
  renderActivityLog();
}

export function setActivityLoadFailed(message: string): void {
  setActivityLoadFailedState(message);
  renderActivityLog();
}

async function refreshActivityLog(silent = false): Promise<void> {
  let task = activityState.refreshInFlight;
  const ownsTask = task === null;
  if (task === null) {
    const requestVersion = activityState.requestVersion;
    task = (async () => {
      const items = await getActivityLog();
      if (requestVersion !== activityState.requestVersion) return;
      const ordered = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
      const snapshot = JSON.stringify(ordered);
      if (snapshot !== activityState.snapshot) setActivityItems(ordered);
    })();
    activityState.refreshInFlight = task;
  }

  try {
    await task;
  } catch (error) {
    if (!silent) {
      setActivityLoadFailed(errorMessage(error));
      throw error;
    }
  } finally {
    if (ownsTask && activityState.refreshInFlight === task) {
      activityState.refreshInFlight = null;
    }
  }
}

async function clearActivityLog(): Promise<void> {
  activityState.actionInProgress = true;
  nextActivityRequestVersion();
  try {
    await clearActivityLogCommand();
    showNotice(t("activity.clearSuccess"));
  } finally {
    activityState.actionInProgress = false;
  }
}

export function setupActivityList(): () => void {
  const unsubscribeActivityCleared = subscribeActivityCleared(() => {
    nextActivityRequestVersion();
    renderActivityLog();
  });
  const unsubscribeLanguage = subscribeLanguage(renderActivityLog);

  const refreshActivityButton = element<HTMLButtonElement>("#refresh-activity");
  const clearActivityButton = element<HTMLButtonElement>("#clear-activity");
  const failedActivityOnlyCheckbox = element<HTMLInputElement>("#activity-failed-only");

  const handleRefresh = () => {
    void withBusy(
      refreshActivityButton,
      () => refreshActivityLog(),
      showNotice,
      t("activity.refreshLog"),
    );
  };
  refreshActivityButton.addEventListener("click", handleRefresh);

  const disposeClearButton = armDestructiveButton(
    clearActivityButton,
    () => t("activity.clearConfirm"),
    () => withBusy(clearActivityButton, clearActivityLog, showNotice),
    showNotice,
  );

  const handleFailedOnlyChange = () => {
    activityState.failedOnly = failedActivityOnlyCheckbox.checked;
    renderActivityLog();
  };
  failedActivityOnlyCheckbox.addEventListener("change", handleFailedOnlyChange);

  const refreshInterval = window.setInterval(() => {
    if (document.visibilityState === "visible" && !activityState.actionInProgress) {
      void refreshActivityLog(true);
    }
  }, 2000);

  const handleVisibilityChange = () => {
    if (document.visibilityState === "visible") void refreshActivityLog(true);
  };
  document.addEventListener("visibilitychange", handleVisibilityChange);

  return () => {
    unsubscribeActivityCleared();
    unsubscribeLanguage();
    refreshActivityButton.removeEventListener("click", handleRefresh);
    disposeClearButton();
    failedActivityOnlyCheckbox.removeEventListener("change", handleFailedOnlyChange);
    window.clearInterval(refreshInterval);
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    nextActivityRequestVersion();
  };
}
