import { element } from "../utils/domUtils";
import { confirmDiscardProviderChanges, closeProviderEditor } from "./ProviderEditor";

const tabCopy: Record<string, { title: string; description: string }> = {
  "tab-status": {
    title: "运行概览",
    description: "查看代理服务、IDE 和 App 的运行状态。",
  },
  "tab-models": {
    title: "模型管理",
    description: "管理 AI 上游服务及其接入 IDE / App 的模型与推理配置。",
  },
  "tab-activity": {
    title: "调用日志",
    description: "查看请求路由、Token 用量与失败详情。",
  },
};

export async function switchTab(targetId: string): Promise<void> {
  const tabTriggers = [...document.querySelectorAll<HTMLButtonElement>(".tab-trigger")];
  const tabPanes = [...document.querySelectorAll<HTMLElement>(".tab-pane")];
  const pageTitle = element<HTMLSpanElement>("#page-title-text");
  const pageDescription = element<HTMLParagraphElement>("#page-description");

  const currentPane = tabPanes.find((pane) => pane.classList.contains("active"));
  if (currentPane?.id === targetId) return;

  const providerFormPanel = element<HTMLElement>("#provider-form-panel");
  if (!providerFormPanel.hidden) {
    if (!(await confirmDiscardProviderChanges())) return;
    void closeProviderEditor(true);
  }

  for (const trigger of tabTriggers) {
    const active = trigger.dataset.target === targetId;
    trigger.classList.toggle("active", active);
    trigger.setAttribute("aria-current", active ? "page" : "false");
  }
  for (const pane of tabPanes) {
    pane.classList.toggle("active", pane.id === targetId);
  }
  const copy = tabCopy[targetId];
  if (copy) {
    pageTitle.textContent = copy.title;
    pageDescription.textContent = copy.description;
  }
  window.scrollTo({ top: 0, behavior: "smooth" });
}

export function setupTabManager(): void {
  const tabTriggers = [...document.querySelectorAll<HTMLButtonElement>(".tab-trigger")];
  for (const trigger of tabTriggers) {
    trigger.addEventListener("click", () => {
      const targetId = trigger.dataset.target;
      if (targetId) switchTab(targetId);
    });
  }
}
