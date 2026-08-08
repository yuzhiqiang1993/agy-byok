export type ClientIntegrationState = "official" | "managed" | "external" | "mismatch" | "conflict" | "unavailable";
export type ClientConfigurationState = "not_enabled" | "matched" | "not_running" | "service_stopped" | "needs_update" | "unavailable";

export interface IdeStatus {
  installed: boolean;
  compatible: boolean;
  ideRunning: boolean;
  proxyRunning: boolean;

  integrationState: ClientIntegrationState;
  settingsPath: string;
  configurationState: ClientConfigurationState;
  canEnableIntegration: boolean;
  canLaunchIde: boolean;
  canDisableIntegration: boolean;
}

export interface AppStatus {
  installed: boolean;
  appRunning: boolean;
  proxyRunning: boolean;
  integrationState: ClientIntegrationState;
  configurationState: ClientConfigurationState;
  canEnableIntegration: boolean;
  canLaunchApp: boolean;
  canDisableIntegration: boolean;
}

export interface CliStatus {
  installed: boolean;
  proxyRunning: boolean;
  integrationState: ClientIntegrationState;
  configurationState: ClientConfigurationState;
  canEnableIntegration: boolean;
  canDisableIntegration: boolean;
}
