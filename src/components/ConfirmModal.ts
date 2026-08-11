import { t } from "../i18n";
import { createModal } from "./common/Modal";

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
    const messageEl = document.createElement("p");
    messageEl.className = "confirm-modal-text";
    messageEl.textContent = message;

    const modal = createModal({
      title: finalTitle,
      body: messageEl,
      dialogClassName: "confirm-modal-dialog",
      okLabel: finalOk,
      cancelLabel: finalCancel,
      onOk: () => {
        resolve(true);
        modal.close();
      },
      onCancel: () => {
        resolve(false);
      }
    });
  });
}
