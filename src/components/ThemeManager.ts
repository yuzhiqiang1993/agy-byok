export function applyTheme(theme: string): void {
  let effectiveTheme = theme;
  if (theme === "system") {
    effectiveTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  document.documentElement.setAttribute("data-theme", effectiveTheme);

  const toggleBtn = document.querySelector("#minimal-theme-toggle");
  if (toggleBtn) {
    const sunIcon = toggleBtn.querySelector(".icon-sun");
    const moonIcon = toggleBtn.querySelector(".icon-moon");
    if (sunIcon) sunIcon.toggleAttribute("hidden", effectiveTheme === "dark");
    if (moonIcon) moonIcon.toggleAttribute("hidden", effectiveTheme !== "dark");
  }
}

export function initThemeManager(): void {
  const savedTheme = localStorage.getItem("agy_theme") || "light";
  applyTheme(savedTheme);

  const toggleBtn = document.querySelector("#minimal-theme-toggle");

  toggleBtn?.addEventListener("click", () => {
    const currentTheme = document.documentElement.getAttribute("data-theme") || "light";
    const nextTheme = currentTheme === "dark" ? "light" : "dark";
    localStorage.setItem("agy_theme", nextTheme);
    applyTheme(nextTheme);
  });
}
