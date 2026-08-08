export type ThemePreference = "system" | "light" | "dark";

const THEME_STORAGE_KEY = "agy_theme";
const SYSTEM_THEME_QUERY = "(prefers-color-scheme: dark)";

export function isThemePreference(value: string | null | undefined): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

export function getThemePreference(): ThemePreference {
  const savedTheme = localStorage.getItem(THEME_STORAGE_KEY);
  return isThemePreference(savedTheme) ? savedTheme : "system";
}

export function applyTheme(theme: ThemePreference): void {
  const effectiveTheme = theme === "system"
    ? window.matchMedia(SYSTEM_THEME_QUERY).matches ? "dark" : "light"
    : theme;
  document.documentElement.dataset.theme = effectiveTheme;
}

export function setThemePreference(theme: ThemePreference): void {
  localStorage.setItem(THEME_STORAGE_KEY, theme);
  applyTheme(theme);
}

export function initThemeManager(): void {
  applyTheme(getThemePreference());
  window.matchMedia(SYSTEM_THEME_QUERY).addEventListener("change", () => {
    if (getThemePreference() === "system") applyTheme("system");
  });
}
