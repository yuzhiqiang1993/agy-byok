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
import { getLanguage, setLanguage, subscribeLanguage, t, type SupportedLocale } from "../i18n";
import { configService } from "../services/configService";
import type { OfficialCompressionProfile, OfficialModelSettings } from "../types/config";

const GEMINI_COMPRESSION_PRESETS: Partial<Record<Exclude<OfficialCompressionProfile, "official" | "custom">, Pick<OfficialModelSettings, "gemini_token_threshold" | "gemini_max_token_limit" | "gemini_max_output_tokens">>> = {
  safe: {
    gemini_token_threshold: 430_000,
    gemini_max_token_limit: 512_000,
    gemini_max_output_tokens: 16_384,
  },
  balanced: {
    gemini_token_threshold: 640_000,
    gemini_max_token_limit: 768_000,
    gemini_max_output_tokens: 16_384,
  },
  aggressive: {
    gemini_token_threshold: 760_000,
    gemini_max_token_limit: 900_000,
    gemini_max_output_tokens: 16_384,
  },
};

const DEFAULT_OFFICIAL_MODEL_SETTINGS: OfficialModelSettings = {
  gemini_compression_profile: "official",
  gemini_token_threshold: 640_000,
  gemini_max_token_limit: 768_000,
  gemini_max_output_tokens: 16_384,
};

function isOfficialCompressionProfile(value: string): value is OfficialCompressionProfile {
  return ["official", "safe", "balanced", "aggressive", "custom"].includes(value);
}

function positiveIntegerOrFallback(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) && value > 0 ? value : fallback;
}

function normalizeOfficialModelSettings(value: Partial<OfficialModelSettings> | undefined): OfficialModelSettings {
  const profile = typeof value?.gemini_compression_profile === "string"
    && isOfficialCompressionProfile(value.gemini_compression_profile)
    ? value.gemini_compression_profile
    : DEFAULT_OFFICIAL_MODEL_SETTINGS.gemini_compression_profile;
  return {
    gemini_compression_profile: profile,
    gemini_token_threshold: positiveIntegerOrFallback(
      value?.gemini_token_threshold,
      DEFAULT_OFFICIAL_MODEL_SETTINGS.gemini_token_threshold,
    ),
    gemini_max_token_limit: positiveIntegerOrFallback(
      value?.gemini_max_token_limit,
      DEFAULT_OFFICIAL_MODEL_SETTINGS.gemini_max_token_limit,
    ),
    gemini_max_output_tokens: positiveIntegerOrFallback(
      value?.gemini_max_output_tokens,
      DEFAULT_OFFICIAL_MODEL_SETTINGS.gemini_max_output_tokens,
    ),
  };
}

function sameOfficialModelSettings(left: OfficialModelSettings, right: OfficialModelSettings): boolean {
  return left.gemini_compression_profile === right.gemini_compression_profile
    && left.gemini_token_threshold === right.gemini_token_threshold
    && left.gemini_max_token_limit === right.gemini_max_token_limit
    && left.gemini_max_output_tokens === right.gemini_max_output_tokens;
}

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

  setupOfficialModelSettings();

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

function setupOfficialModelSettings(): void {
  const profileSelect = document.querySelector<HTMLSelectElement>("#settings-gemini-compression-profile");
  const thresholdInput = document.querySelector<HTMLInputElement>("#settings-gemini-threshold");
  const hardLimitInput = document.querySelector<HTMLInputElement>("#settings-gemini-hard-limit");
  const outputReserveInput = document.querySelector<HTMLInputElement>("#settings-gemini-output-reserve");
  const saveButton = document.querySelector<HTMLButtonElement>("#save-gemini-settings");
  const source = document.querySelector<HTMLElement>("#settings-gemini-source");
  if (!profileSelect || !thresholdInput || !hardLimitInput || !outputReserveInput || !saveButton || !source) {
    return;
  }

  let savedSettings = normalizeOfficialModelSettings(store.config.official_model_settings);
  let draftSettings = { ...savedSettings };

  const presetValues = (profile: OfficialCompressionProfile) => {
    if (profile === "safe" || profile === "balanced" || profile === "aggressive") {
      return GEMINI_COMPRESSION_PRESETS[profile];
    }
    return undefined;
  };

  const draftIsValid = (): boolean => {
    if (draftSettings.gemini_compression_profile === "official") return true;
    const threshold = draftSettings.gemini_token_threshold;
    const hardLimit = draftSettings.gemini_max_token_limit;
    const outputReserve = draftSettings.gemini_max_output_tokens;
    return Number.isInteger(threshold)
      && Number.isInteger(hardLimit)
      && Number.isInteger(outputReserve)
      && threshold > 0
      && hardLimit > 0
      && outputReserve > 0
      && threshold < hardLimit
      && hardLimit <= 1_048_576
      && outputReserve < hardLimit;
  };

  const render = (writeValues: boolean) => {
    const profile = draftSettings.gemini_compression_profile;
    profileSelect.value = profile;
    const preset = presetValues(profile);
    if (preset) {
      draftSettings = { ...draftSettings, ...preset };
    }
    const fieldsAreEditable = profile === "custom";
    thresholdInput.disabled = !fieldsAreEditable;
    hardLimitInput.disabled = !fieldsAreEditable;
    outputReserveInput.disabled = !fieldsAreEditable;
    if (writeValues) {
      const showValues = profile !== "official";
      thresholdInput.value = showValues ? String(draftSettings.gemini_token_threshold) : "";
      hardLimitInput.value = showValues ? String(draftSettings.gemini_max_token_limit) : "";
      outputReserveInput.value = showValues ? String(draftSettings.gemini_max_output_tokens) : "";
    }
    source.textContent = profile === "official"
      ? t("settings.geminiCompressionOfficialHint")
      : t("settings.geminiCompressionLocalHint");
    saveButton.disabled = !store.configLoaded
      || sameOfficialModelSettings(savedSettings, draftSettings)
      || !draftIsValid();
  };

  const syncFromStore = () => {
    // 配置加载完成后同步；用户正在编辑时不覆盖未保存草稿。
    if (document.activeElement === profileSelect
      || document.activeElement === thresholdInput
      || document.activeElement === hardLimitInput
      || document.activeElement === outputReserveInput) {
      return;
    }
    savedSettings = normalizeOfficialModelSettings(store.config.official_model_settings);
    draftSettings = { ...savedSettings };
    render(true);
  };

  profileSelect.addEventListener("change", () => {
    const value = profileSelect.value;
    if (!isOfficialCompressionProfile(value)) return;
    draftSettings.gemini_compression_profile = value;
    render(true);
  });

  const updateCustomValue = (
    input: HTMLInputElement,
    field: "gemini_token_threshold" | "gemini_max_token_limit" | "gemini_max_output_tokens",
  ) => {
    const parsed = Number(input.value);
    draftSettings = {
      ...draftSettings,
      [field]: Number.isInteger(parsed) && parsed > 0 ? parsed : 0,
    };
    render(false);
  };
  thresholdInput.addEventListener("input", () => updateCustomValue(thresholdInput, "gemini_token_threshold"));
  hardLimitInput.addEventListener("input", () => updateCustomValue(hardLimitInput, "gemini_max_token_limit"));
  outputReserveInput.addEventListener("input", () => updateCustomValue(outputReserveInput, "gemini_max_output_tokens"));

  saveButton.addEventListener("click", () => {
    if (!store.configLoaded) {
      showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
      return;
    }
    if (!draftIsValid()) {
      showNotice(t("settings.geminiCompressionInvalid"), "error");
      return;
    }
    saveButton.disabled = true;
    void configService.saveConfig({
      ...store.config,
      official_model_settings: draftSettings,
    }).then((savedConfig) => {
      store.setConfig(savedConfig);
      savedSettings = normalizeOfficialModelSettings(savedConfig.official_model_settings);
      draftSettings = { ...savedSettings };
      render(true);
      showNotice(t("settings.geminiCompressionSaved"), "success");
    }).catch((error: unknown) => {
      render(false);
      showNotice(t("settings.geminiCompressionSaveFailed", { message: errorMessage(error) }), "error");
    });
  });

  store.subscribeConfig(syncFromStore);
  subscribeLanguage(() => render(false));
  render(true);
}
