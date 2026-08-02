import { clientErrorMessage } from "./displayUtils";

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

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function setButtonUnavailable(button: HTMLButtonElement, unavailable: boolean): void {
  button.dataset.unavailable = String(unavailable);
  if (button.dataset.busy !== "true") button.disabled = unavailable;
}

export async function withBusy(
  button: HTMLButtonElement,
  action: () => Promise<void>,
  busyLabel = "处理中…",
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

export function clientActionButtons(client: "ide" | "app" | "cli"): HTMLButtonElement[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(`#${client}-actions button`));
}

export async function withClientBusy<T>(
  button: HTMLButtonElement,
  client: "ide" | "app" | "cli",
  action: () => Promise<T>,
  busyLabel = "处理中…",
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
