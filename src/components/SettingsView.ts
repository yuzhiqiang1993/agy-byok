import { setupAboutSettings } from "./settings/AboutSettings";
import { setupAppearanceSettings } from "./settings/AppearanceSettings";
import { setupDataSettings } from "./settings/DataSettings";
import { setupProxyPortSettings } from "./settings/ProxyPortSettings";
import { setupSettingsNavigation } from "./settings/SettingsNavigation";

export function setupSettingsView(): void {
  setupSettingsNavigation();
  setupAppearanceSettings();
  setupProxyPortSettings();
  setupDataSettings();
  setupAboutSettings();
}
