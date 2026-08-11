import { t, type TranslationKey } from "../i18n";

const ERROR_TRANSLATIONS: Partial<Record<string, TranslationKey>> = {
  host_status_failed: "overview.loadFailed",
  host_modify_failed: "overview.hostModifyError",
  host_launch_failed: "overview.hostModifyError",
  provider_catalog_failed: "models.catalogFetchFailed",
  official_models_host_not_installed: "models.officialHostNotInstalled",
  official_models_host_not_running: "models.officialHostNotRunning",

  official_models_fetch_failed: "models.officialSourcesFailed",
  config_save_failed: "errors.configSaveFailed",
  proxy_reconfigure_failed: "errors.proxyReconfigureFailed",
  proxy_start_failed: "errors.proxyStartFailed",
  proxy_stop_failed: "errors.proxyStopFailed",
  path_open_failed: "errors.pathOpenFailed",
  config_path_failed: "errors.configPathFailed",
  config_dir_open_failed: "errors.configDirOpenFailed",
  external_url_invalid: "errors.externalUrlInvalid",
  external_url_open_failed: "errors.externalUrlOpenFailed",
};

export function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const translation = ERROR_TRANSLATIONS[message];
  return translation ? t(translation) : message;
}
