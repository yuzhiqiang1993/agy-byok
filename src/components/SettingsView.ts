import { showNotice } from "./NoticeBar";
import { store } from "../store/appStore";
import { clearActivityLog } from "../controllers/activityController";
import { setProxyPort } from "../controllers/proxyController";
import {
  refreshIde,
  refreshApp,
  refreshCli,
  openConfigDir,
  openExternalUrl as openExternalUrlCommand,
} from "../controllers/hostController";
import { errorMessage } from "../utils/domUtils";
import { applyTheme } from "./ThemeManager";
import { getLanguage, setLanguage, t, type SupportedLocale } from "../i18n";

export function setupSettingsView(): void {
  const navItems = [...document.querySelectorAll<HTMLButtonElement>(".settings-nav-item")];
  const panes = [...document.querySelectorAll<HTMLElement>(".settings-pane")];

  // 语言选择器联动
  const langSelect = document.querySelector<HTMLSelectElement>("#settings-language-select");
  if (langSelect) {
    langSelect.value = getLanguage();
    langSelect.addEventListener("change", () => {
      const selectedLang = langSelect.value as SupportedLocale;
      setLanguage(selectedLang);
      showNotice(`${t("settings.languageTitle")}: ${langSelect.options[langSelect.selectedIndex].text}`, "success");
    });
  }

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
      const labels: Record<string, string> = {
        system: t("header.themeSystem"),
        light: t("header.themeLight"),
        dark: t("header.themeDark"),
      };
      showNotice(t("settings.themeChanged", { theme: labels[val] ?? val }));
    });
  }

  // 端口保存与 Dirty 智能联动 + 代理自动重启逻辑
  const savePortBtn = document.querySelector<HTMLButtonElement>("#save-proxy-port");
  const portInput = document.querySelector<HTMLInputElement>("#settings-proxy-port");

  if (savePortBtn && portInput) {
    const updatePortState = () => {
      const configAvailable = store.configLoaded;
      const currentVal = Number(portInput.value.trim());
      const savedPort = store.config.proxy_port;
      const isDirty = currentVal !== savedPort;
      const isValid = Number.isInteger(currentVal) && currentVal >= 1024 && currentVal <= 65535;
      portInput.disabled = !configAvailable;
      savePortBtn.disabled = !configAvailable || !isDirty || !isValid;
    };

    portInput.value = String(store.config.proxy_port);
    updatePortState();
    store.subscribeConfig(() => {
      if (document.activeElement !== portInput) {
        portInput.value = String(store.config.proxy_port);
      }
      updatePortState();
    });

    portInput.addEventListener("input", updatePortState);
    portInput.addEventListener("change", updatePortState);

    const handleSavePort = async () => {
      if (!store.configLoaded) {
        showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
        return;
      }
      const newPort = Number(portInput.value.trim());
      if (!Number.isInteger(newPort) || newPort < 1024 || newPort > 65535) {
        showNotice(t("settings.invalidPort"), "error");
        return;
      }

      const isRunning = store.proxyStatus?.state === "running";
      try {
        savePortBtn.disabled = true;
        if (isRunning) {
          showNotice(t("settings.restartingProxy", { port: newPort }));
        }
        const status = await setProxyPort(newPort);
        const savedPort = status.port;
        portInput.value = String(savedPort);
        const hostRefreshResults = await Promise.allSettled([refreshIde(), refreshApp(), refreshCli()]);
        updatePortState();

        if (hostRefreshResults.some((result) => result.status === "rejected")) {
          showNotice(t("settings.portSavedHostRefreshFailed", { port: savedPort }), "error");
        } else {
          showNotice(
            t(isRunning ? "settings.portSavedRunning" : "settings.portSavedStopped", { port: savedPort }),
            "success",
          );
        }
      } catch (err) {
        showNotice(t("settings.portSaveFailed", { message: errorMessage(err) }), "error");
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
      await clearActivityLog();
      showNotice(t("activity.clearSuccess"));
    } catch (err) {
      showNotice(t("settings.clearLogsFailed", { message: errorMessage(err) }), "error");
    }
  });

  // 打开配置目录按钮 (Finder)
  const openConfigDirBtn = document.querySelector("#open-config-dir");
  const aboutCardDir = document.querySelector("#about-card-dir");
  const openDirHandler = async () => {
    try {
      await openConfigDir();
      showNotice(t("settings.configDirOpened"), "success");
    } catch (err) {
      showNotice(t("settings.configDirOpenFailed", { message: errorMessage(err) }), "error");
    }
  };

  openConfigDirBtn?.addEventListener("click", openDirHandler);
  aboutCardDir?.addEventListener("click", openDirHandler);

  // 打开外部 GitHub 链接
  const openExternalUrl = async (url: string, label: string) => {
    try {
      await openExternalUrlCommand(url);
      showNotice(t("settings.externalOpened", { label }));
    } catch {
      window.open(url, "_blank");
    }
  };

  const aboutCardGithub = document.querySelector("#about-card-github");
  aboutCardGithub?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993/agy-byok", t("settings.cardGithub"));
  });

  const aboutCardAuthor = document.querySelector("#about-card-author");
  aboutCardAuthor?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993", t("settings.cardAuthor"));
  });

  const aboutCardFeedback = document.querySelector("#about-card-feedback");
  aboutCardFeedback?.addEventListener("click", () => {
    void openExternalUrl("https://github.com/yuzhiqiang1993/agy-byok/issues", t("settings.cardFeedback"));
  });
}
