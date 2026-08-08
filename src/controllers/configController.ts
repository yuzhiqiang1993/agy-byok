import type { AppConfig } from "../types/config";
import { configService } from "../services/configService";
import { store } from "../store/appStore";
import { t } from "../i18n";

type ConfigUpdate = (current: AppConfig) => AppConfig;

let configUpdateQueue: Promise<void> = Promise.resolve();

export function updateConfig(update: ConfigUpdate): Promise<AppConfig> {
  const execute = async (): Promise<AppConfig> => {
    if (!store.configLoaded) {
      throw new Error(store.configLoadError ?? t("overview.loadFailed"));
    }
    const savedConfig = await configService.saveConfig(update(store.config));
    store.setConfig(savedConfig);
    return savedConfig;
  };
  const result = configUpdateQueue.then(execute, execute);
  configUpdateQueue = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}
