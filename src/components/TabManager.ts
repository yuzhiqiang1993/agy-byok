import { element } from "../utils/domUtils";
import { confirmDiscardProviderChanges, closeProviderEditor } from "./ProviderEditor";
import { t, subscribeLanguage, type TranslationKey } from "../i18n";

let activeTabId = "tab-status";

const tabKeys: Record<string, { titleKey: TranslationKey; descKey: TranslationKey }> = {
  "tab-status": {
    titleKey: "overview.pageTitle",
    descKey: "overview.pageDesc",
  },
  "tab-models": {
    titleKey: "models.title",
    descKey: "models.subtitle",
  },
  "tab-activity": {
    titleKey: "activity.title",
    descKey: "activity.subtitle",
  },
  "tab-compression": {
    titleKey: "settings.antigravityHeader",
    descKey: "settings.antigravitySubtitle",
  },
  "tab-settings": {
    titleKey: "settings.title",
    descKey: "settings.subtitle",
  },
};

function updatePageHeader(targetId: string = activeTabId): void {
  const pageTitle = element<HTMLSpanElement>("#page-title-text");
  const pageDescription = element<HTMLParagraphElement>("#page-description");

  const keys = tabKeys[targetId];
  if (keys) {
    pageTitle.textContent = t(keys.titleKey);
    pageDescription.textContent = t(keys.descKey);
  }
}

// Re-update page header whenever display language changes
subscribeLanguage(() => {
  updatePageHeader(activeTabId);
});

export async function switchTab(targetId: string): Promise<void> {
  const tabTriggers = [...document.querySelectorAll<HTMLButtonElement>(".tab-trigger")];
  const tabPanes = [...document.querySelectorAll<HTMLElement>(".tab-pane")];

  const currentPane = tabPanes.find((pane) => pane.classList.contains("active"));
  if (currentPane?.id === targetId) return;

  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  if (!providerFormPanel.hidden) {
    if (!(await confirmDiscardProviderChanges())) return;
    void closeProviderEditor(true);
  }

  activeTabId = targetId;
  for (const trigger of tabTriggers) {
    const active = trigger.dataset.target === targetId;
    trigger.classList.toggle("active", active);
    trigger.setAttribute("aria-current", active ? "page" : "false");
  }
  for (const pane of tabPanes) {
    pane.classList.toggle("active", pane.id === targetId);
  }
  updatePageHeader(targetId);
  window.scrollTo({ top: 0, behavior: "smooth" });
}

export function setupTabManager(): void {
  const tabTriggers = [...document.querySelectorAll<HTMLButtonElement>(".tab-trigger")];
  for (const trigger of tabTriggers) {
    trigger.addEventListener("click", () => {
      const targetId = trigger.dataset.target;
      if (targetId) switchTab(targetId);
    });
  }
}
