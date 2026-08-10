import { element } from "../utils/domUtils";

let noticeTimer: number | null = null;

export interface NoticeAction {
  label: string;
  onClick: () => void;
}

export function showNotice(
  message: string,
  kind: "success" | "error" | "info" = "success",
  action?: NoticeAction,
): void {
  const notice = element<HTMLDivElement>("#notice");
  const noticeText = element<HTMLSpanElement>("#notice-text");
  const actionBtn = element<HTMLButtonElement>("#notice-action-btn");
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeText.textContent = message;
  notice.className = `notice ${kind}`;

  if (action) {
    actionBtn.hidden = false;
    actionBtn.textContent = action.label;
    actionBtn.onclick = () => {
      dismissNotice();
      action.onClick();
    };
  } else {
    actionBtn.hidden = true;
    actionBtn.onclick = null;
  }

  notice.hidden = false;
  noticeTimer = window.setTimeout(
    () => {
      dismissNotice();
    },
    action ? 10000 : kind === "error" ? 8000 : 4000,
  );
}

export function dismissNotice(): void {
  const notice = element<HTMLDivElement>("#notice");
  const actionBtn = element<HTMLButtonElement>("#notice-action-btn");
  if (noticeTimer !== null) window.clearTimeout(noticeTimer);
  noticeTimer = null;
  actionBtn.hidden = true;
  actionBtn.onclick = null;
  notice.hidden = true;
}

export function setupNoticeBar(): void {
  element<HTMLButtonElement>("#dismiss-notice").addEventListener("click", dismissNotice);
}
