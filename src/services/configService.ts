import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "../types/config";

export const configService = {
  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (config: AppConfig) => invoke<AppConfig>("save_config", { config }),
};
