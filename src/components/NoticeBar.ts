import { element } from "../utils/domUtils";

let noticeTimer: number | null = null;

export function showNotice(message: string, kind: "success" | "error" = "success"): void {
  const notice = element<HTMLDivElement>("#notice");
  const noticeText = element<HTMLSpanElement>("#notice-text");
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeText.textContent = message;
  notice.className = `notice ${kind}`;
  notice.hidden = false;
  noticeTimer = window.setTimeout(() => {
    notice.hidden = true;
    noticeTimer = null;
  }, kind === "error" ? 8000 : 4000);
}

export function dismissNotice(): void {
  const notice = element<HTMLDivElement>("#notice");
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeTimer = null;
  notice.hidden = true;
}

export function setupNoticeBar(): void {
  element<HTMLButtonElement>("#dismiss-notice").addEventListener("click", dismissNotice);
}
