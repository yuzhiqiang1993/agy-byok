import { t } from "../i18n";
import { visibleFocusableElements } from "../utils/domUtils";

export function showRawConfigModal(modelName: string, rawConfig: unknown): void {
  const returnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  const modal = document.createElement("div");
  modal.className = "provider-modal";

  const backdrop = document.createElement("div");
  backdrop.className = "provider-modal-backdrop";

  const dialog = document.createElement("section");
  dialog.className = "provider-modal-dialog raw-config-dialog";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.setAttribute("aria-labelledby", "raw-config-title");
  dialog.setAttribute("aria-describedby", "raw-config-description");

  const header = document.createElement("header");
  header.className = "provider-modal-header";

  const heading = document.createElement("div");
  heading.className = "raw-config-heading";
  const title = document.createElement("strong");
  title.id = "raw-config-title";
  title.textContent = `${modelName} · ${t("models.viewRawConfig")}`;
  const description = document.createElement("p");
  description.id = "raw-config-description";
  description.textContent = t("models.rawConfigDescription");
  heading.append(title, description);

  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "provider-modal-close";
  closeButton.setAttribute("aria-label", t("modal.close"));
  closeButton.title = t("modal.closeWithShortcut");
  closeButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`;
  header.append(heading, closeButton);

  const body = document.createElement("div");
  body.className = "provider-modal-body raw-config-body";
  const sectionTitle = document.createElement("strong");
  sectionTitle.className = "raw-config-section-title";
  sectionTitle.textContent = t("models.rawConfigFullJson");
  const pre = document.createElement("pre");
  pre.className = "raw-config-json";
  pre.tabIndex = 0;
  pre.textContent = JSON.stringify(rawConfig, null, 2) ?? String(rawConfig);
  body.append(sectionTitle, pre);

  const footer = document.createElement("footer");
  footer.className = "reasoning-modal-footer";
  const doneButton = document.createElement("button");
  doneButton.type = "button";
  doneButton.className = "primary";
  doneButton.textContent = t("modal.close");
  footer.append(doneButton);

  dialog.append(header, body, footer);
  modal.append(backdrop, dialog);

  const close = (): void => {
    window.removeEventListener("keydown", handleKeyDown);
    document.body.classList.remove("modal-open");
    modal.remove();
    if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
  };

  function handleKeyDown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = visibleFocusableElements(dialog);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  backdrop.addEventListener("click", close);
  closeButton.addEventListener("click", close);
  doneButton.addEventListener("click", close);
  document.body.append(modal);
  document.body.classList.add("modal-open");
  window.addEventListener("keydown", handleKeyDown);
  window.setTimeout(() => closeButton.focus(), 0);
}
