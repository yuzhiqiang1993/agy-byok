import { invoke } from "@tauri-apps/api/core";
import type { IdeStatus, AppStatus, CliStatus } from "../types/host";

export const hostService = {
  discoverIde: () => invoke<IdeStatus>("discover_ide"),
  discoverApp: () => invoke<AppStatus>("discover_app"),
  discoverCli: () => invoke<CliStatus>("discover_cli"),
  enableIdeIntegration: () => invoke<IdeStatus>("enable_ide_integration"),
  disableIdeIntegration: () => invoke<IdeStatus>("disable_ide_integration"),
  launchIde: () => invoke<void>("launch_ide"),
  enableAppIntegration: () => invoke<AppStatus>("enable_app_integration"),
  disableAppIntegration: () => invoke<AppStatus>("disable_app_integration"),
  launchApp: () => invoke<void>("launch_app"),
  setCustomIdePath: (path: string) => invoke<IdeStatus>("set_custom_ide_path", { path }),
  resetCustomIdePath: () => invoke<IdeStatus>("reset_custom_ide_path"),
  setCustomAppPath: (path: string) => invoke<AppStatus>("set_custom_app_path", { path }),
  resetCustomAppPath: () => invoke<AppStatus>("reset_custom_app_path"),
  enableCliIntegration: () => invoke<CliStatus>("enable_cli_integration"),
  disableCliIntegration: () => invoke<CliStatus>("disable_cli_integration"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  getConfigPath: () => invoke<string>("get_config_path"),
  setNativeLocale: (locale: string) => invoke<void>("set_native_locale", { locale }),
  openConfigDir: () => invoke<void>("open_config_dir"),
  openExternalUrl: (url: string) => invoke<void>("open_external_url", { url }),
};
