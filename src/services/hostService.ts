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
  enableCliIntegration: () => invoke<CliStatus>("enable_cli_integration"),
  disableCliIntegration: () => invoke<CliStatus>("disable_cli_integration"),
};
