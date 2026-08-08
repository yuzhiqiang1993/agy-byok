import { element } from "../utils/domUtils";
import { t } from "../i18n";

export function confirmHostAction(
  message: string,
  title?: string,
  okLabel?: string,
  cancelLabel?: string,
): Promise<boolean> {
  const finalTitle = title ?? t("modal.confirmTitle");
  const finalOk = okLabel ?? t("modal.confirmOk");
  const finalCancel = cancelLabel ?? t("models.cancel");

  return new Promise<boolean>((resolve) => {
    const modal = element<HTMLDivElement>("#confirm-modal");
    const backdrop = element<HTMLDivElement>("#confirm-modal-backdrop");
    const titleEl = element<HTMLElement>("#confirm-modal-title");
    const messageEl = element<HTMLParagraphElement>("#confirm-modal-message");
    const closeBtn = element<HTMLButtonElement>("#close-confirm-modal");
    const cancelBtn = element<HTMLButtonElement>("#cancel-confirm-modal");
    const okBtn = element<HTMLButtonElement>("#ok-confirm-modal");

    titleEl.textContent = finalTitle;
    messageEl.textContent = message;
    okBtn.textContent = finalOk;
    cancelBtn.textContent = finalCancel;

    let cleanup = (): void => {};

    const handleOk = (): void => {
      cleanup();
      resolve(true);
    };

    const handleCancel = (): void => {
      cleanup();
      resolve(false);
    };

    const handleKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.preventDefault();
        handleCancel();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = [closeBtn, cancelBtn, okBtn];
      const active = document.activeElement;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && (active === first || !modal.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !modal.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    };

    cleanup = (): void => {
      modal.hidden = true;
      document.body.classList.remove("modal-open");
      backdrop.removeEventListener("click", handleCancel);
      closeBtn.removeEventListener("click", handleCancel);
      cancelBtn.removeEventListener("click", handleCancel);
      okBtn.removeEventListener("click", handleOk);
      window.removeEventListener("keydown", handleKeyDown);
    };

    backdrop.addEventListener("click", handleCancel);
    closeBtn.addEventListener("click", handleCancel);
    cancelBtn.addEventListener("click", handleCancel);
    okBtn.addEventListener("click", handleOk);
    window.addEventListener("keydown", handleKeyDown);

    modal.hidden = false;
    document.body.classList.add("modal-open");
    okBtn.focus();
  });
}
