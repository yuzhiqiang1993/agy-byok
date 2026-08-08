import { getLanguage, setLanguage, t, type SupportedLocale } from "../../i18n";
import { showNotice } from "../NoticeBar";
import {
  getThemePreference,
  isThemePreference,
  setThemePreference,
  type ThemePreference,
} from "../ThemeManager";

function setupLanguageSelector(): void {
  const select = document.querySelector<HTMLSelectElement>("#settings-language-select");
  if (!select) return;
  select.value = getLanguage();
  select.addEventListener("change", () => {
    setLanguage(select.value as SupportedLocale);
    showNotice(
      `${t("settings.languageTitle")}: ${select.options[select.selectedIndex].text}`,
      "success",
    );
  });
}

function setupThemeSelector(): void {
  const buttons = [...document.querySelectorAll<HTMLButtonElement>(".theme-btn")];
  const syncButtons = (theme: string) => {
    for (const button of buttons) button.classList.toggle("active", button.dataset.themeVal === theme);
  };
  syncButtons(getThemePreference());
  for (const button of buttons) {
    button.addEventListener("click", () => {
      const theme = button.dataset.themeVal;
      if (!isThemePreference(theme)) return;
      setThemePreference(theme);
      syncButtons(theme);
      const labels: Record<ThemePreference, string> = {
        system: t("header.themeSystem"),
        light: t("header.themeLight"),
        dark: t("header.themeDark"),
      };
      showNotice(t("settings.themeChanged", { theme: labels[theme] }));
    });
  }
}

export function setupAppearanceSettings(): void {
  setupLanguageSelector();
  setupThemeSelector();
}
