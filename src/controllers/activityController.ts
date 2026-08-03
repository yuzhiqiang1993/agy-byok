import type { ActivityItem } from "../types/activity";
import { activityService } from "../services/activityService";
import { clearActivityItems } from "../features/activity/activityState";

type ActivityClearedListener = () => void;
const activityClearedListeners = new Set<ActivityClearedListener>();

export function getActivityLog(): Promise<ActivityItem[]> {
  return activityService.getLog();
}

export async function clearActivityLog(): Promise<void> {
  await activityService.clearLog();
  clearActivityItems();
  activityClearedListeners.forEach((listener) => listener());
}

export function subscribeActivityCleared(listener: ActivityClearedListener): () => void {
  activityClearedListeners.add(listener);
  return () => activityClearedListeners.delete(listener);
}
