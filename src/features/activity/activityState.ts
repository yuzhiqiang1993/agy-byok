import type { ActivityItem } from "../../types/activity";

export interface ActivityState {
  items: ActivityItem[];
  snapshot: string;
  loadError: string | null;
  failedOnly: boolean;
  requestVersion: number;
  actionInProgress: boolean;
  refreshInFlight: Promise<void> | null;
}

export const activityState: ActivityState = {
  items: [],
  snapshot: "",
  loadError: null,
  failedOnly: false,
  requestVersion: 0,
  actionInProgress: false,
  refreshInFlight: null,
};

export function setActivityItems(items: ActivityItem[]): void {
  activityState.loadError = null;
  activityState.items = [...items].sort((left, right) => right.timestampMs - left.timestampMs);
  activityState.snapshot = JSON.stringify(activityState.items);
}

export function setActivityLoadFailed(message: string): void {
  activityState.items = [];
  activityState.snapshot = "";
  activityState.loadError = message;
}

export function clearActivityItems(): void {
  activityState.items = [];
  activityState.snapshot = "";
  activityState.loadError = null;
}

export function nextActivityRequestVersion(): number {
  activityState.requestVersion += 1;
  return activityState.requestVersion;
}
