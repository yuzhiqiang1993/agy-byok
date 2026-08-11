import { t } from "../../i18n";

export interface NoticeAction {
  label: string;
  onClick: () => void;
}

export type NoticeKind = "success" | "error" | "info" | "warning";

interface NoticeOptions {
  message: string;
  kind?: NoticeKind;
  duration?: number; // 0 means persistent
  action?: NoticeAction;
}

class NoticeManager {
  private container: HTMLDivElement | null = null;

  private initContainer() {
    if (this.container) return;
    this.container = document.createElement("div");
    this.container.className = "agy-notice-container";
    document.body.append(this.container);
  }

  public show(options: NoticeOptions | string, kind: NoticeKind = "info", action?: NoticeAction) {
    this.initContainer();

    const opts: NoticeOptions = typeof options === "string" ? {
      message: options,
      kind,
      action
    } : options;

    const noticeType = opts.kind ?? "info";
    const duration = opts.duration ?? (opts.action ? 10000 : noticeType === "error" ? 8000 : 4000);

    const toast = document.createElement("div");
    toast.className = `agy-notice agy-notice-${noticeType}`;
    
    // Icon
    const icon = document.createElement("span");
    icon.className = "agy-notice-icon";
    if (noticeType === "success") {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><path d="m9 11 3 3L22 4"/></svg>`;
    } else if (noticeType === "error") {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/></svg>`;
    } else if (noticeType === "warning") {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>`;
    } else {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`;
    }
    toast.append(icon);

    // Text
    const text = document.createElement("span");
    text.className = "agy-notice-text";
    text.textContent = opts.message;
    toast.append(text);

    // Action
    if (opts.action) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "agy-notice-action";
      btn.textContent = opts.action.label;
      btn.addEventListener("click", () => {
        opts.action?.onClick();
        closeToast();
      });
      toast.append(btn);
    }

    // Close button
    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "agy-notice-close";
    closeBtn.setAttribute("aria-label", t("modal.dismissNotice"));
    closeBtn.title = t("modal.dismissNotice");
    closeBtn.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`;
    closeBtn.addEventListener("click", () => closeToast());
    toast.append(closeBtn);

    let isClosed = false;
    const closeToast = () => {
      if (isClosed) return;
      isClosed = true;
      toast.classList.add("agy-notice-closing");
      
      let removed = false;
      const removeNode = () => {
        if (removed) return;
        removed = true;
        toast.remove();
      };

      toast.addEventListener("animationend", removeNode);
      setTimeout(removeNode, 220);
    };

    this.container!.append(toast);

    // Force reflow for animation
    void toast.offsetWidth;
    toast.classList.add("agy-notice-showing");

    if (duration > 0) {
      setTimeout(closeToast, duration);
    }
  }

  public success(message: string, action?: NoticeAction) {
    this.show(message, "success", action);
  }

  public error(message: string, action?: NoticeAction) {
    this.show(message, "error", action);
  }

  public info(message: string, action?: NoticeAction) {
    this.show(message, "info", action);
  }

  public warning(message: string, action?: NoticeAction) {
    this.show(message, "warning", action);
  }
}

export const Notice = new NoticeManager();
