import { t } from "../../i18n";

export interface ModalOptions {
  title: string | HTMLElement;
  subtitle?: string | HTMLElement;
  titleExtras?: HTMLElement[];
  body: HTMLElement;
  footer?: HTMLElement;
  okLabel?: string;
  cancelLabel?: string;
  onOk?: () => void | Promise<void>;
  onCancel?: () => void;
  dialogClassName?: string;
  bodyClassName?: string;
  closeOnBackdropClick?: boolean;
}

export interface ModalInstance {
  element: HTMLElement;
  close: () => void;
}

export function createModal(options: ModalOptions): ModalInstance {
  const overlay = document.createElement("div");
  overlay.className = "agy-modal";

  const backdrop = document.createElement("div");
  backdrop.className = "agy-modal-backdrop";

  const dialog = document.createElement("section");
  dialog.className = `agy-modal-dialog ${options.dialogClassName ?? ""}`.trim();
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.tabIndex = -1;

  // Header
  const header = document.createElement("header");
  header.className = "agy-modal-header";

  const heading = document.createElement("div");
  heading.className = "agy-modal-heading";

  const titleRow = document.createElement("div");
  titleRow.className = "agy-modal-title-row";
  
  const titleEl = document.createElement("strong");
  titleEl.className = "agy-modal-title";
  if (typeof options.title === "string") {
    titleEl.textContent = options.title;
  } else {
    titleEl.append(options.title);
  }
  titleRow.append(titleEl);

  if (options.titleExtras) {
    titleRow.append(...options.titleExtras);
  }

  heading.append(titleRow);

  if (options.subtitle) {
    const subtitleEl = document.createElement("p");
    subtitleEl.className = "agy-modal-subtitle";
    if (typeof options.subtitle === "string") {
      subtitleEl.textContent = options.subtitle;
    } else {
      subtitleEl.append(options.subtitle);
    }
    heading.append(subtitleEl);
  }

  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "agy-modal-close";
  closeButton.setAttribute("aria-label", t("modal.close"));
  closeButton.title = t("modal.closeWithShortcut");
  closeButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`;
  
  header.append(heading, closeButton);

  // Body
  const body = document.createElement("div");
  body.className = `agy-modal-body ${options.bodyClassName ?? ""}`.trim();
  body.append(options.body);

  // Footer
  let footer: HTMLElement | undefined;
  if (options.footer) {
    footer = options.footer;
    footer.classList.add("agy-modal-footer");
  } else if (options.onOk || options.onCancel || options.okLabel || options.cancelLabel) {
    footer = document.createElement("footer");
    footer.className = "agy-modal-footer";
    
    // Always render cancel if onCancel or cancelLabel is provided
    if (options.onCancel || options.cancelLabel) {
      const cancelBtn = document.createElement("button");
      cancelBtn.type = "button";
      cancelBtn.className = "secondary";
      cancelBtn.textContent = options.cancelLabel ?? t("models.cancel"); // Changed to common cancel if needed, using models.cancel for now
      cancelBtn.addEventListener("click", () => {
        if (options.onCancel) options.onCancel();
        close();
      });
      footer.append(cancelBtn);
    }
    
    if (options.onOk || options.okLabel) {
      const okBtn = document.createElement("button");
      okBtn.type = "button";
      okBtn.className = "primary";
      okBtn.textContent = options.okLabel ?? t("modal.confirmOk");
      okBtn.addEventListener("click", () => {
        if (options.onOk) void options.onOk();
      });
      footer.append(okBtn);
    }
  }

  dialog.append(header, body);
  if (footer) dialog.append(footer);
  overlay.append(backdrop, dialog);

  let isClosed = false;
  const close = () => {
    if (isClosed) return;
    isClosed = true;
    overlay.remove();
    document.removeEventListener("keydown", onKeyDown);
    if (document.body.style.overflow === "hidden") {
        document.body.style.overflow = "";
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      if (options.onCancel) options.onCancel();
      close();
    }
  };

  closeButton.addEventListener("click", () => {
    if (options.onCancel) options.onCancel();
    close();
  });

  if (options.closeOnBackdropClick !== false) {
    backdrop.addEventListener("click", (e) => {
      // Only close if clicking the backdrop itself, not its children
      if (e.target === backdrop) {
          if (options.onCancel) options.onCancel();
          close();
      }
    });
  }

  document.body.append(overlay);
  document.body.style.overflow = "hidden";
  document.addEventListener("keydown", onKeyDown);
  dialog.focus();

  return {
    element: overlay,
    close,
  };
}
