import { element } from "../utils/domUtils";
import { confirmDiscardProviderChanges, closeProviderEditor } from "./ProviderEditor";

const tabCopy: Record<string, { title: string; description: string }> = {
  "tab-status": {
    title: "运行概览",
    description: "按四步配置模型、启动代理，并选择要启用代理模式的 IDE、App 或 CLI。",
  },
  "tab-models": {
    title: "模型管理",
    description: "第 1 步：添加上游服务，获取模型列表并保存需要使用的模型。",
  },
  "tab-activity": {
    title: "调用日志",
    description: "查看请求路由、Token 用量与失败详情。",
  },
  "tab-settings": {
    title: "应用设置",
    description: "管理本地代理服务端口、配置文件与应用关于信息。",
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
