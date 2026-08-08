import { isTranslationKey, subscribeLanguage, t } from "../i18n";
import { errorMessage } from "./errorUtils";

// 通知展示由组件层传入，工具层不持有通知 DOM 或定时器。
type NoticeHandler = (message: string, kind?: "success" | "error") => void;
type ButtonLabel = () => string;

// 处理完成后按当前语言恢复声明式按钮文案，避免回退到切换前的标签。
function translatedButtonLabel(button: HTMLButtonElement, fallback: string): string {
  const key = button.dataset.i18n;
  return key && isTranslationKey(key) ? t(key) : fallback;
}

export function element<T extends HTMLElement>(selector: string): T {
  const value = document.querySelector<T>(selector);
  if (!value) throw new Error(`Missing element: ${selector}`);
  return value;
}

export function visibleFocusableElements(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(
    'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], summary, [tabindex]:not([tabindex="-1"])',
  )].filter((item) => (
    !item.hidden
    && !item.closest("[inert]")
    && item.getClientRects().length > 0
  ));
}

export function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
}

export async function withBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  notify: NoticeHandler,
  busyLabel = t("models.processing"),
): Promise<void> {
  if (button.dataset.busy === "true") return;
  const label = button.textContent ?? "";
  button.dataset.busy = "true";
  button.disabled = true;
  button.textContent = busyLabel;
  try {
    await action();
  } catch (error) {
    notify(errorMessage(error), "error");
  } finally {
    button.dataset.busy = "false";
    button.textContent = translatedButtonLabel(button, label);
    button.disabled = button.dataset.unavailable === "true"
      || button.dataset.bulkBusy === "true";
  }
}

export function armDestructiveButton(
  button: HTMLButtonElement,
  confirmLabel: ButtonLabel,
  action: () => Promise<void>,
  notify: NoticeHandler,
  beforeArm?: () => string | null,
): () => void {
  const initialLabel = button.textContent ?? "";
  let armed = false;
  let resetTimer: number | null = null;
  const reset = () => {
    armed = false;
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    resetTimer = null;
    button.dataset.armed = "false";
    button.textContent = translatedButtonLabel(button, initialLabel);
    button.classList.remove("danger-confirm");
  };

  const handleClick = () => {
    if (!armed) {
      const blocker = beforeArm?.();
      if (blocker) {
        notify(blocker, "error");
        return;
      }
      armed = true;
      button.dataset.armed = "true";
      button.textContent = confirmLabel();
      button.classList.add("danger-confirm");
      resetTimer = window.setTimeout(reset, 4000);
      return;
    }

    const blocker = beforeArm?.();
    if (blocker) {
      reset();
      notify(blocker, "error");
      return;
    }
    void action().finally(reset);
  };
  button.addEventListener("click", handleClick);
  const unsubscribeLanguage = subscribeLanguage(() => {
    if (armed && button.dataset.busy !== "true") button.textContent = confirmLabel();
  });
  return () => {
    button.removeEventListener("click", handleClick);
    unsubscribeLanguage();
    reset();
  };
}

function clientActionButtons(client: "ide" | "app" | "cli"): HTMLButtonElement[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(`#${client}-actions button`));
}

export async function withClientBusy<T>(
  button: HTMLButtonElement,
  client: "ide" | "app" | "cli",
  action: () => Promise<T>,
  notify: NoticeHandler,
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
    notify(errorMessage(error), "error");
  } finally {
    buttons.forEach((item) => {
      item.dataset.busy = "false";
      item.textContent = translatedButtonLabel(item, labels.get(item) ?? "");
      item.disabled = item.dataset.unavailable === "true";
    });
  }
  return result;
}
