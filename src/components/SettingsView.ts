import { invoke } from "@tauri-apps/api/core";
import { showNotice } from "./NoticeBar";
import { store } from "../store/appStore";
import { configService } from "../services/configService";
import { proxyService } from "../services/proxyService";
import { renderProxy } from "./ProxyCard";
import { refreshIde, refreshApp, refreshCli } from "./HostRefresh";
import { errorMessage } from "../utils/domUtils";
import { applyTheme } from "./ThemeManager";

export function setupSettingsView(): void {
  const navItems = [...document.querySelectorAll<HTMLButtonElement>(".settings-nav-item")];
  const panes = [...document.querySelectorAll<HTMLElement>(".settings-pane")];

  for (const item of navItems) {
    item.addEventListener("click", () => {
      const targetId = item.dataset.settingsTarget;
      if (!targetId) return;

      for (const nav of navItems) {
        nav.classList.toggle("active", nav.dataset.settingsTarget === targetId);
      }
      for (const p of panes) {
        p.classList.toggle("active", p.id === targetId);
      }
    });
  }

  // 主题 Segmented 按钮处理
  const themeBtns = [...document.querySelectorAll<HTMLButtonElement>(".theme-btn")];
  const syncThemeButtons = (themeVal: string) => {
    for (const btn of themeBtns) {
      const active = btn.dataset.themeVal === themeVal;
      btn.classList.toggle("active", active);
    }
  };

  const savedTheme = localStorage.getItem("agy_theme") || "system";
  syncThemeButtons(savedTheme);

  for (const btn of themeBtns) {
    btn.addEventListener("click", () => {
      const val = btn.dataset.themeVal ?? "system";
      localStorage.setItem("agy_theme", val);
      applyTheme(val);
      syncThemeButtons(val);
      const labels: Record<string, string> = { system: "跟随系统", light: "浅色模式", dark: "深色模式" };
      showNotice(`已切换应用主题为：${labels[val] ?? val}`);
    });
  }

  // 端口保存与 Dirty 智能联动 + 代理自动重启逻辑
  const savePortBtn = document.querySelector<HTMLButtonElement>("#save-proxy-port");
  const portInput = document.querySelector<HTMLInputElement>("#settings-proxy-port");

  if (savePortBtn && portInput) {
    const updatePortState = () => {
      const currentVal = Number(portInput.value.trim());
      const savedPort = store.config?.proxy_port ?? 54321;
      const isDirty = currentVal !== savedPort;
      const isValid = Number.isInteger(currentVal) && currentVal >= 1024 && currentVal <= 65535;
      savePortBtn.disabled = !isDirty || !isValid;
    };

    if (store.config) {
      portInput.value = String(store.config.proxy_port);
    }
    updatePortState();

    portInput.addEventListener("input", updatePortState);
    portInput.addEventListener("change", updatePortState);

    const handleSavePort = async () => {
      const newPort = Number(portInput.value.trim());
      if (!Number.isInteger(newPort) || newPort < 1024 || newPort > 65535) {
        showNotice("请输入 1024 - 65535 之间的合法端口号", "error");
        return;
      }
      if (!store.config) return;

      const isRunning = store.proxyStatus?.state === "running";

      try {
        savePortBtn.disabled = true;
        store.config.proxy_port = newPort;
        await configService.saveConfig(store.config);

        if (isRunning) {
          showNotice(`正在重启代理服务绑定新端口 ${newPort}...`);
          await proxyService.stop();
          const newStatus = await proxyService.start();
          renderProxy(newStatus);
          await Promise.all([refreshIde(), refreshApp(), refreshCli()]);
          showNotice(`代理端口已成功保存为 ${newPort}，服务已在关联网口自动重启！`, "success");
        } else {
          renderProxy({ state: "stopped", address: null });
          showNotice(`代理端口已成功保存为 ${newPort}。下次启动代理服务时生效。`, "success");
        }

        updatePortState();
      } catch (err) {
        showNotice(`保存或重启代理服务失败：${errorMessage(err)}`, "error");
        updatePortState();
      }
    };

    savePortBtn.addEventListener("click", () => void handleSavePort());
    portInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !savePortBtn.disabled) {
        e.preventDefault();
        void handleSavePort();
      }
    });
  }

  // 数据管理: 清空日志
  const settingsClearLogsBtn = document.querySelector("#settings-clear-logs-btn");
  settingsClearLogsBtn?.addEventListener("click", async () => {
    try {
      await invoke<void>("clear_activity_log");
      showNotice("内存调用日志已成功清空");
    } catch (err) {
      showNotice(`清空日志失败: ${errorMessage(err)}`, "error");
    }
  });

  // 打开配置目录按钮 (Finder)
  const openConfigDirBtn = document.querySelector("#open-config-dir");
  const aboutCardDir = document.querySelector("#about-card-dir");
  const openDirHandler = async () => {
    try {
      await invoke<void>("open_config_dir");
      showNotice("已在 Finder 中打开配置目录", "success");
    } catch (err) {
      showNotice(`无法打开配置目录: ${errorMessage(err)}`, "error");
    }
  };

  openConfigDirBtn?.addEventListener("click", openDirHandler);
  aboutCardDir?.addEventListener("click", openDirHandler);

  // 打开外部 GitHub 链接
  const openExternalUrl = async (url: string, label: string) => {
    try {
      await invoke<void>("open_external_url", { url });
      showNotice(`已在浏览器中打开: ${label}`);
    } catch {
      window.open(url, "_blank");
    }
  };

  const aboutCardGithub = document.querySelector("#about-card-github");
  aboutCardGithub?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993/agy-byok", "开源仓库");
  });

  const aboutCardAuthor = document.querySelector("#about-card-author");
  aboutCardAuthor?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993", "开发者主页");
  });

  const aboutCardFeedback = document.querySelector("#about-card-feedback");
  aboutCardFeedback?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993/agy-byok/issues", "意见反馈");
  });
}
