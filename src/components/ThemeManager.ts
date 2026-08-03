export function applyTheme(theme: string): void {
  let effectiveTheme = theme;
  if (theme === "system") {
    effectiveTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  document.documentElement.setAttribute("data-theme", effectiveTheme);
}

export function initThemeManager(): void {
  const savedTheme = localStorage.getItem("agy_theme") || "light";
  applyTheme(savedTheme);
}
