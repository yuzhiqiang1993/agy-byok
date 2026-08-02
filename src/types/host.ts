export type ClientIntegrationState = "official" | "managed" | "external" | "mismatch" | "conflict" | "unavailable";
export type ClientConfigurationState = "not_enabled" | "matched" | "not_running" | "service_stopped" | "needs_update" | "checking" | "unavailable" | "active";

export interface IdeStatus {
  installed: boolean;
  compatible: boolean;
  ideRunning: boolean;
  proxyRunning: boolean;

  state: "not_installed" | "vendor_original" | "patched" | "modified" | "incompatible";
  appPath: string;
  appVersion: string | null;
  extensionVersion: string | null;
  extensionSha256: string | null;
  message: string;
  integrationState: ClientIntegrationState;
  settingsPath: string;
  integrationMessage: string;
  configurationState: ClientConfigurationState;
  configurationMessage: string;
  canEnableIntegration: boolean;
  canLaunchIde: boolean;
  canDisableIntegration: boolean;
}

export interface AppStatus {
  installed: boolean;
  appRunning: boolean;
  proxyRunning: boolean;
  appPath: string;
  appVersion: string | null;
  lsPath: string;
  integrationState: ClientIntegrationState;
  integrationMessage: string;
  configurationState: ClientConfigurationState;
  configurationMessage: string;
  configuredEndpoint: string | null;
  canEnableIntegration: boolean;
  canLaunchApp: boolean;
  canDisableIntegration: boolean;
}

export interface CliStatus {
  installed: boolean;
  proxyRunning: boolean;
  cliPath: string | null;
  integrationState: ClientIntegrationState;
  integrationMessage: string;
  configurationState: ClientConfigurationState;
  configurationMessage: string;
  configuredEndpoint: string | null;
  canEnableIntegration: boolean;
  canDisableIntegration: boolean;
}
