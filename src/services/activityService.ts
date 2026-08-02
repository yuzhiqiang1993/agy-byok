import { invoke } from "@tauri-apps/api/core";
import type { ActivityItem } from "../types/activity";

export const activityService = {
  getLog: () => invoke<ActivityItem[]>("get_activity_log"),
  clearLog: () => invoke<void>("clear_activity_log"),
};
