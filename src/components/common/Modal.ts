import { t } from "../../i18n";
import { visibleFocusableElements } from "../../utils/domUtils";

let nextModalId = 0;

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
  onClosed?: () => void;
  dialogClassName?: string;
  bodyClassName?: string;
  closeOnBackdropClick?: boolean;
}

export interface ModalInstance {
  element: HTMLElement;
  close: () => void;
  setBusy: (busy: boolean, busyLabel?: string) => void;
}

export function createModal(options: ModalOptions): ModalInstance {
  const modalId = `agy-modal-${++nextModalId}`;
  const returnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  const previousBodyOverflow = document.body.style.overflow;
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
  titleEl.id = `${modalId}-title`;
  titleEl.className = "agy-modal-title";
  if (typeof options.title === "string") {
    titleEl.textContent = options.title;
  } else {
    titleEl.append(options.title);
  }
  titleRow.append(titleEl);
  dialog.setAttribute("aria-labelledby", titleEl.id);

  if (options.titleExtras) {
    titleRow.append(...options.titleExtras);
  }

  heading.append(titleRow);

  if (options.subtitle) {
    const subtitleEl = document.createElement("p");
    subtitleEl.id = `${modalId}-subtitle`;
    subtitleEl.className = "agy-modal-subtitle";
    if (typeof options.subtitle === "string") {
      subtitleEl.textContent = options.subtitle;
    } else {
      subtitleEl.append(options.subtitle);
    }
    heading.append(subtitleEl);
    dialog.setAttribute("aria-describedby", subtitleEl.id);
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
  let okButton: HTMLButtonElement | null = null;
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
      cancelBtn.addEventListener("click", () => cancel());
      footer.append(cancelBtn);
    }
    
    if (options.onOk || options.okLabel) {
      okButton = document.createElement("button");
      okButton.type = "button";
      okButton.className = "primary";
      okButton.textContent = options.okLabel ?? t("modal.confirmOk");
      okButton.addEventListener("click", () => {
        if (options.onOk) void options.onOk();
      });
      footer.append(okButton);
    }
  }

  dialog.append(header, body);
  if (footer) dialog.append(footer);
  overlay.append(backdrop, dialog);

  let isClosed = false;
  let isBusy = false;
  // busy 结束后恢复每个控件原本的禁用状态，避免误启用业务上不可操作的字段。
  const disabledStates = new Map<
    HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement,
    boolean
  >();
  const close = () => {
    if (isClosed) return;
    isClosed = true;
    overlay.remove();
    document.removeEventListener("keydown", onKeyDown);
    document.body.style.overflow = previousBodyOverflow;
    options.onClosed?.();
    if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
  };

  const cancel = () => {
    if (isBusy || isClosed) return;
    options.onCancel?.();
    close();
  };

  const setBusy = (busy: boolean, busyLabel?: string) => {
    if (isClosed || isBusy === busy) return;
    isBusy = busy;
    dialog.setAttribute("aria-busy", String(busy));
    const controls = dialog.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement>(
      "button, input, select, textarea",
    );
    if (busy) {
      disabledStates.clear();
      controls.forEach((control) => {
        disabledStates.set(control, control.disabled);
        control.disabled = true;
      });
    } else {
      controls.forEach((control) => {
        control.disabled = disabledStates.get(control) ?? control.disabled;
      });
      disabledStates.clear();
    }
    if (okButton) {
      okButton.textContent = busy && busyLabel
        ? busyLabel
        : options.okLabel ?? t("modal.confirmOk");
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      if (isBusy) return;
      e.preventDefault();
      cancel();
      return;
    }
    if (e.key !== "Tab") return;
    // 对话框打开期间将键盘焦点约束在内部，避免误操作被遮罩覆盖的页面。
    const focusable = visibleFocusableElements(dialog);
    if (focusable.length === 0) {
      e.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || !dialog.contains(active))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (active === last || !dialog.contains(active))) {
      e.preventDefault();
      first.focus();
    }
  };

  closeButton.addEventListener("click", cancel);

  if (options.closeOnBackdropClick !== false) {
    backdrop.addEventListener("click", (e) => {
      // Only close if clicking the backdrop itself, not its children
      if (e.target === backdrop) {
        cancel();
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
    setBusy,
  };
}
