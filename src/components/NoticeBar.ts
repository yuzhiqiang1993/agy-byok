import { Notice } from "./common/Notice";

export interface NoticeAction {
  label: string;
  onClick: () => void;
}

export function showNotice(
  message: string,
  kind: "success" | "error" | "info" = "success",
  action?: NoticeAction,
): void {
  Notice.show({
    message,
    kind,
    action
  });
}

export function dismissNotice(): void {
  // Common notice automatically dismisses itself.
  // There is no global dismissNotice for all toast notices since there can be multiple.
}

export function setupNoticeBar(): void {
  // No-op. Notice.ts manages its own DOM automatically.
}
