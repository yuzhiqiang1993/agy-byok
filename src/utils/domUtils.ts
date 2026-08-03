import { clientErrorMessage } from "./displayUtils";
import { t } from "../i18n";
import { errorMessage } from "./errorUtils";

export { errorMessage };

let noticeTimer: number | null = null;

export function showNotice(message: string, kind: "success" | "error" = "success"): void {
  const notice = document.querySelector<HTMLDivElement>("#notice");
  const noticeText = document.querySelector<HTMLSpanElement>("#notice-text");
  if (!notice || !noticeText) return;
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeText.textContent = message;
  notice.className = `notice ${kind}`;
  notice.hidden = false;
  noticeTimer = window.setTimeout(() => {
    notice.hidden = true;
    noticeTimer = null;
  }, kind === "error" ? 8000 : 4000);
}

export function element<T extends HTMLElement>(selector: string): T {
  const value = document.querySelector<T>(selector);
  if (!value) throw new Error(`Missing element: ${selector}`);
  return value;
}



export function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
}

export async function withBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = t("models.processing"),
): Promise<void> {
  if (button.dataset.busy === "true") return;
  const label = button.textContent;
  button.dataset.busy = "true";
  button.disabled = true;
  button.textContent = busyLabel;
  try {
    await action();
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    button.dataset.busy = "false";
    button.textContent = label;
    button.disabled = button.dataset.unavailable === "true"
      || button.dataset.bulkBusy === "true";
  }
}

export function armDestructiveButton(
  button: HTMLButtonElement,
  confirmLabel: string,
  action: () => Promise<void>,
  beforeArm?: () => string | null,
): void {
  const initialLabel = button.textContent ?? "Delete";
  let armed = false;
  let resetTimer: number | null = null;
  const reset = () => {
    armed = false;
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    resetTimer = null;
    button.textContent = initialLabel;
    button.classList.remove("danger-confirm");
  };

  button.addEventListener("click", () => {
    if (!armed) {
      const blocker = beforeArm?.();
      if (blocker) {
        showNotice(blocker, "error");
        return;
      }
      armed = true;
      button.textContent = confirmLabel;
      button.classList.add("danger-confirm");
      resetTimer = window.setTimeout(reset, 4000);
      return;
    }

    const blocker = beforeArm?.();
    if (blocker) {
      reset();
      showNotice(blocker, "error");
      return;
    }
    void action().finally(reset);
  });
}

export function clientActionButtons(client: "ide" | "app" | "cli"): HTMLButtonElement[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(`#${client}-actions button`));
}

export async function withClientBusy<T>(
  button: HTMLButtonElement,
  client: "ide" | "app" | "cli",
  action: () => Promise<T>,
  busyLabel = t("models.processing"),
): Promise<T | undefined> {
  if (button.dataset.busy === "true") return undefined;
  const buttons = clientActionButtons(client);
  if (buttons.some((item) => item.dataset.busy === "true")) return undefined;
  const labels = new Map(buttons.map((item) => [item, item.textContent ?? ""]));
  buttons.forEach((item) => {
    item.dataset.busy = "true";
    item.disabled = true;
  });
  button.textContent = busyLabel;
  let result: T | undefined;
  try {
    result = await action();
  } catch (error) {
    showNotice(clientErrorMessage(error), "error");
  } finally {
    buttons.forEach((item) => {
      item.dataset.busy = "false";
      item.textContent = labels.get(item) ?? item.textContent;
      item.disabled = item.dataset.unavailable === "true";
    });
  }
  return result;
}
