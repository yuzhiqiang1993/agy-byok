export function switchSettingsPane(targetId: string): void {
  const navItems = [...document.querySelectorAll<HTMLButtonElement>(".settings-nav-item")];
  const panes = [...document.querySelectorAll<HTMLElement>(".settings-pane")];
  for (const nav of navItems) {
    nav.classList.toggle("active", nav.dataset.settingsTarget === targetId);
  }
  for (const pane of panes) pane.classList.toggle("active", pane.id === targetId);
}

export function setupSettingsNavigation(): void {
  const navItems = [...document.querySelectorAll<HTMLButtonElement>(".settings-nav-item")];
  for (const item of navItems) {
    item.addEventListener("click", () => {
      const targetId = item.dataset.settingsTarget;
      if (targetId) switchSettingsPane(targetId);
    });
  }
}
