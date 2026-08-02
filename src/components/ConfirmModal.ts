import { element } from "../utils/domUtils";

export function confirmHostAction(
  message: string,
  title = "确认操作",
  okLabel = "确认继续",
  cancelLabel = "取消",
): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    const modal = element<HTMLDivElement>("#confirm-modal");
    const backdrop = element<HTMLDivElement>("#confirm-modal-backdrop");
    const titleEl = element<HTMLElement>("#confirm-modal-title");
    const messageEl = element<HTMLParagraphElement>("#confirm-modal-message");
    const closeBtn = element<HTMLButtonElement>("#close-confirm-modal");
    const cancelBtn = element<HTMLButtonElement>("#cancel-confirm-modal");
    const okBtn = element<HTMLButtonElement>("#ok-confirm-modal");

    titleEl.textContent = title;
    messageEl.textContent = message;
    okBtn.textContent = okLabel;
    cancelBtn.textContent = cancelLabel;

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
      } else if (event.key === "Enter") {
        event.preventDefault();
        handleOk();
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
