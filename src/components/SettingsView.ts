import { showNotice } from "./NoticeBar";
import { confirmHostAction } from "./ConfirmModal";
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
import type {
  ClaudeCompressionProfile,
  CustomModelCompressionProfile,
  OfficialCompressionProfile,
  OfficialModelSettings,
} from "../types/config";

type CompressionSettingsDraft = OfficialModelSettings;

type PercentField =
  | "gemini_token_threshold_percent"
  | "gemini_max_token_limit_percent"
  | "gemini_max_output_tokens_percent"
  | "claude_token_threshold_percent"
  | "claude_max_token_limit_percent"
  | "claude_max_output_tokens_percent"
  | "custom_model_token_threshold_percent"
  | "custom_model_max_token_limit_percent"
  | "custom_model_max_output_tokens_percent";

type PercentFields = readonly [PercentField, PercentField, PercentField];

const DEFAULT_COMPRESSION_SETTINGS: CompressionSettingsDraft = {
  gemini_compression_profile: "official",
  claude_compression_profile: "official",
  custom_model_compression_profile: "balanced",
  gemini_token_threshold_percent: 61,
  gemini_max_token_limit_percent: 73,
  gemini_max_output_tokens_percent: 2,
  claude_token_threshold_percent: 61,
  claude_max_token_limit_percent: 73,
  claude_max_output_tokens_percent: 2,
  custom_model_token_threshold_percent: 61,
  custom_model_max_token_limit_percent: 73,
  custom_model_max_output_tokens_percent: 2,
};

function isOfficialCompressionProfile(value: string): value is OfficialCompressionProfile {
  return ["official", "safe", "balanced", "aggressive", "custom"].includes(value);
}

function isClaudeCompressionProfile(value: string): value is ClaudeCompressionProfile {
  return ["official", "safe", "balanced", "aggressive", "custom"].includes(value);
}

function isCustomModelCompressionProfile(value: string): value is CustomModelCompressionProfile {
  return ["safe", "balanced", "aggressive", "custom"].includes(value);
}

function percentageOrFallback(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 100
    ? value
    : fallback;
}

function normalizeCompressionSettings(
  value: Partial<OfficialModelSettings> | undefined,
): CompressionSettingsDraft {
  const geminiProfile = typeof value?.gemini_compression_profile === "string"
    && isOfficialCompressionProfile(value.gemini_compression_profile)
    ? value.gemini_compression_profile
    : DEFAULT_COMPRESSION_SETTINGS.gemini_compression_profile;
  const claudeProfile = typeof value?.claude_compression_profile === "string"
    && isClaudeCompressionProfile(value.claude_compression_profile)
    ? value.claude_compression_profile
    : DEFAULT_COMPRESSION_SETTINGS.claude_compression_profile;
  const customModelProfile = typeof value?.custom_model_compression_profile === "string"
    && isCustomModelCompressionProfile(value.custom_model_compression_profile)
    ? value.custom_model_compression_profile
    : DEFAULT_COMPRESSION_SETTINGS.custom_model_compression_profile;

  return {
    gemini_compression_profile: geminiProfile,
    claude_compression_profile: claudeProfile,
    custom_model_compression_profile: customModelProfile,
    gemini_token_threshold_percent: percentageOrFallback(
      value?.gemini_token_threshold_percent,
      DEFAULT_COMPRESSION_SETTINGS.gemini_token_threshold_percent,
    ),
    gemini_max_token_limit_percent: percentageOrFallback(
      value?.gemini_max_token_limit_percent,
      DEFAULT_COMPRESSION_SETTINGS.gemini_max_token_limit_percent,
    ),
    gemini_max_output_tokens_percent: percentageOrFallback(
      value?.gemini_max_output_tokens_percent,
      DEFAULT_COMPRESSION_SETTINGS.gemini_max_output_tokens_percent,
    ),
    claude_token_threshold_percent: percentageOrFallback(
      value?.claude_token_threshold_percent,
      DEFAULT_COMPRESSION_SETTINGS.claude_token_threshold_percent,
    ),
    claude_max_token_limit_percent: percentageOrFallback(
      value?.claude_max_token_limit_percent,
      DEFAULT_COMPRESSION_SETTINGS.claude_max_token_limit_percent,
    ),
    claude_max_output_tokens_percent: percentageOrFallback(
      value?.claude_max_output_tokens_percent,
      DEFAULT_COMPRESSION_SETTINGS.claude_max_output_tokens_percent,
    ),
    custom_model_token_threshold_percent: percentageOrFallback(
      value?.custom_model_token_threshold_percent,
      DEFAULT_COMPRESSION_SETTINGS.custom_model_token_threshold_percent,
    ),
    custom_model_max_token_limit_percent: percentageOrFallback(
      value?.custom_model_max_token_limit_percent,
      DEFAULT_COMPRESSION_SETTINGS.custom_model_max_token_limit_percent,
    ),
    custom_model_max_output_tokens_percent: percentageOrFallback(
      value?.custom_model_max_output_tokens_percent,
      DEFAULT_COMPRESSION_SETTINGS.custom_model_max_output_tokens_percent,
    ),
  };
}

interface CompressionSettingsChanges {
  gemini: boolean;
  claude: boolean;
  customModel: boolean;
}

function compressionSettingsChanges(
  left: CompressionSettingsDraft,
  right: CompressionSettingsDraft,
): CompressionSettingsChanges {
  return {
    gemini: left.gemini_compression_profile !== right.gemini_compression_profile
      || left.gemini_token_threshold_percent !== right.gemini_token_threshold_percent
      || left.gemini_max_token_limit_percent !== right.gemini_max_token_limit_percent
      || left.gemini_max_output_tokens_percent !== right.gemini_max_output_tokens_percent,
    claude: left.claude_compression_profile !== right.claude_compression_profile
      || left.claude_token_threshold_percent !== right.claude_token_threshold_percent
      || left.claude_max_token_limit_percent !== right.claude_max_token_limit_percent
      || left.claude_max_output_tokens_percent !== right.claude_max_output_tokens_percent,
    customModel: left.custom_model_compression_profile !== right.custom_model_compression_profile
      || left.custom_model_token_threshold_percent !== right.custom_model_token_threshold_percent
      || left.custom_model_max_token_limit_percent !== right.custom_model_max_token_limit_percent
      || left.custom_model_max_output_tokens_percent !== right.custom_model_max_output_tokens_percent,
  };
}

function sameCompressionSettings(
  left: CompressionSettingsDraft,
  right: CompressionSettingsDraft,
): boolean {
  const changes = compressionSettingsChanges(left, right);
  return !changes.gemini && !changes.claude && !changes.customModel;
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

  setupCompressionSettings();

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

function setupCompressionSettings(): void {
  const geminiProfileSelect = document.querySelector<HTMLSelectElement>("#settings-gemini-compression-profile");
  const claudeProfileSelect = document.querySelector<HTMLSelectElement>("#settings-claude-compression-profile");
  const customModelProfileSelect = document.querySelector<HTMLSelectElement>("#settings-custom-model-compression-profile");
  const geminiParameters = document.querySelector<HTMLElement>("#settings-gemini-custom-parameters");
  const claudeParameters = document.querySelector<HTMLElement>("#settings-claude-custom-parameters");
  const customModelParameters = document.querySelector<HTMLElement>("#settings-custom-model-custom-parameters");
  const geminiThresholdPercentInput = document.querySelector<HTMLInputElement>("#settings-gemini-threshold-percent");
  const geminiHardLimitPercentInput = document.querySelector<HTMLInputElement>("#settings-gemini-hard-limit-percent");
  const geminiOutputReservePercentInput = document.querySelector<HTMLInputElement>("#settings-gemini-output-reserve-percent");
  const claudeThresholdPercentInput = document.querySelector<HTMLInputElement>("#settings-claude-threshold-percent");
  const claudeHardLimitPercentInput = document.querySelector<HTMLInputElement>("#settings-claude-hard-limit-percent");
  const claudeOutputReservePercentInput = document.querySelector<HTMLInputElement>("#settings-claude-output-reserve-percent");
  const customModelThresholdPercentInput = document.querySelector<HTMLInputElement>("#settings-custom-model-threshold-percent");
  const customModelHardLimitPercentInput = document.querySelector<HTMLInputElement>("#settings-custom-model-hard-limit-percent");
  const customModelOutputReservePercentInput = document.querySelector<HTMLInputElement>("#settings-custom-model-output-reserve-percent");
  const resetButton = document.querySelector<HTMLButtonElement>("#reset-gemini-settings");
  const saveButton = document.querySelector<HTMLButtonElement>("#save-gemini-settings");
  const source = document.querySelector<HTMLElement>("#settings-gemini-source");
  if (!geminiProfileSelect
    || !claudeProfileSelect
    || !customModelProfileSelect
    || !geminiParameters
    || !claudeParameters
    || !customModelParameters
    || !geminiThresholdPercentInput
    || !geminiHardLimitPercentInput
    || !geminiOutputReservePercentInput
    || !claudeThresholdPercentInput
    || !claudeHardLimitPercentInput
    || !claudeOutputReservePercentInput
    || !customModelThresholdPercentInput
    || !customModelHardLimitPercentInput
    || !customModelOutputReservePercentInput
    || !resetButton
    || !saveButton
    || !source) {
    return;
  }

  let savedSettings = normalizeCompressionSettings(store.config.official_model_settings);
  let draftSettings = { ...savedSettings };

  const percentValuesAreValid = (
    threshold: number,
    hardLimit: number,
    outputReserve: number,
  ): boolean => Number.isInteger(threshold)
    && Number.isInteger(hardLimit)
    && Number.isInteger(outputReserve)
    && threshold >= 1
    && hardLimit >= 1
    && outputReserve >= 1
    && threshold <= 100
    && hardLimit <= 100
    && outputReserve <= 100
    && threshold < hardLimit
    && outputReserve < hardLimit
    && threshold + outputReserve <= hardLimit;

  const geminiPercentFields: PercentFields = [
    "gemini_token_threshold_percent",
    "gemini_max_token_limit_percent",
    "gemini_max_output_tokens_percent",
  ];
  const claudePercentFields: PercentFields = [
    "claude_token_threshold_percent",
    "claude_max_token_limit_percent",
    "claude_max_output_tokens_percent",
  ];
  const customModelPercentFields: PercentFields = [
    "custom_model_token_threshold_percent",
    "custom_model_max_token_limit_percent",
    "custom_model_max_output_tokens_percent",
  ];
  const restoreInvalidPercentages = ([threshold, hardLimit, outputReserve]: PercentFields) => {
    if (percentValuesAreValid(
      draftSettings[threshold],
      draftSettings[hardLimit],
      draftSettings[outputReserve],
    )) {
      return;
    }
    draftSettings = {
      ...draftSettings,
      [threshold]: DEFAULT_COMPRESSION_SETTINGS[threshold],
      [hardLimit]: DEFAULT_COMPRESSION_SETTINGS[hardLimit],
      [outputReserve]: DEFAULT_COMPRESSION_SETTINGS[outputReserve],
    };
  };

  const draftIsValid = (): boolean => percentValuesAreValid(
    draftSettings.gemini_token_threshold_percent,
    draftSettings.gemini_max_token_limit_percent,
    draftSettings.gemini_max_output_tokens_percent,
  )
    && percentValuesAreValid(
      draftSettings.claude_token_threshold_percent,
      draftSettings.claude_max_token_limit_percent,
      draftSettings.claude_max_output_tokens_percent,
    )
    && percentValuesAreValid(
      draftSettings.custom_model_token_threshold_percent,
      draftSettings.custom_model_max_token_limit_percent,
      draftSettings.custom_model_max_output_tokens_percent,
    );

  const render = (writeValues: boolean) => {
    const configAvailable = store.configLoaded;
    geminiProfileSelect.value = draftSettings.gemini_compression_profile;
    claudeProfileSelect.value = draftSettings.claude_compression_profile;
    customModelProfileSelect.value = draftSettings.custom_model_compression_profile;
    geminiProfileSelect.disabled = !configAvailable;
    claudeProfileSelect.disabled = !configAvailable;
    customModelProfileSelect.disabled = !configAvailable;

    const geminiCustom = draftSettings.gemini_compression_profile === "custom";
    const claudeCustom = draftSettings.claude_compression_profile === "custom";
    const customModelCustom = draftSettings.custom_model_compression_profile === "custom";
    geminiParameters.hidden = !geminiCustom;
    claudeParameters.hidden = !claudeCustom;
    customModelParameters.hidden = !customModelCustom;
    geminiThresholdPercentInput.disabled = !configAvailable || !geminiCustom;
    geminiHardLimitPercentInput.disabled = !configAvailable || !geminiCustom;
    geminiOutputReservePercentInput.disabled = !configAvailable || !geminiCustom;
    claudeThresholdPercentInput.disabled = !configAvailable || !claudeCustom;
    claudeHardLimitPercentInput.disabled = !configAvailable || !claudeCustom;
    claudeOutputReservePercentInput.disabled = !configAvailable || !claudeCustom;
    customModelThresholdPercentInput.disabled = !configAvailable || !customModelCustom;
    customModelHardLimitPercentInput.disabled = !configAvailable || !customModelCustom;
    customModelOutputReservePercentInput.disabled = !configAvailable || !customModelCustom;

    if (writeValues) {
      geminiThresholdPercentInput.value = String(draftSettings.gemini_token_threshold_percent);
      geminiHardLimitPercentInput.value = String(draftSettings.gemini_max_token_limit_percent);
      geminiOutputReservePercentInput.value = String(draftSettings.gemini_max_output_tokens_percent);
      claudeThresholdPercentInput.value = String(draftSettings.claude_token_threshold_percent);
      claudeHardLimitPercentInput.value = String(draftSettings.claude_max_token_limit_percent);
      claudeOutputReservePercentInput.value = String(draftSettings.claude_max_output_tokens_percent);
      customModelThresholdPercentInput.value = String(draftSettings.custom_model_token_threshold_percent);
      customModelHardLimitPercentInput.value = String(draftSettings.custom_model_max_token_limit_percent);
      customModelOutputReservePercentInput.value = String(draftSettings.custom_model_max_output_tokens_percent);
    }

    source.textContent = t("settings.compressionStrategyStatus");
    const isDefaultDraft = sameCompressionSettings(draftSettings, DEFAULT_COMPRESSION_SETTINGS);
    resetButton.disabled = !configAvailable || isDefaultDraft;
    saveButton.disabled = !configAvailable
      || sameCompressionSettings(savedSettings, draftSettings)
      || !draftIsValid();
  };

  const syncFromStore = () => {
    const incomingSettings = normalizeCompressionSettings(store.config.official_model_settings);
    if (sameCompressionSettings(savedSettings, incomingSettings)) {
      render(false);
      return;
    }
    if (!sameCompressionSettings(savedSettings, draftSettings)) {
      return;
    }
    savedSettings = incomingSettings;
    draftSettings = { ...incomingSettings };
    render(true);
  };

  geminiProfileSelect.addEventListener("change", () => {
    const value = geminiProfileSelect.value;
    if (!isOfficialCompressionProfile(value)) return;
    if (value !== "custom") restoreInvalidPercentages(geminiPercentFields);
    draftSettings.gemini_compression_profile = value;
    render(true);
  });
  claudeProfileSelect.addEventListener("change", () => {
    const value = claudeProfileSelect.value;
    if (!isClaudeCompressionProfile(value)) return;
    if (value !== "custom") restoreInvalidPercentages(claudePercentFields);
    draftSettings.claude_compression_profile = value;
    render(true);
  });
  customModelProfileSelect.addEventListener("change", () => {
    const value = customModelProfileSelect.value;
    if (!isCustomModelCompressionProfile(value)) return;
    if (value !== "custom") restoreInvalidPercentages(customModelPercentFields);
    draftSettings.custom_model_compression_profile = value;
    render(true);
  });

  const updatePercentValue = (input: HTMLInputElement, field: PercentField) => {
    const parsed = Number(input.value);
    draftSettings = {
      ...draftSettings,
      [field]: Number.isInteger(parsed) && parsed >= 1 && parsed <= 100 ? parsed : 0,
    };
    render(false);
  };
  geminiThresholdPercentInput.addEventListener("input", () => updatePercentValue(geminiThresholdPercentInput, "gemini_token_threshold_percent"));
  geminiHardLimitPercentInput.addEventListener("input", () => updatePercentValue(geminiHardLimitPercentInput, "gemini_max_token_limit_percent"));
  geminiOutputReservePercentInput.addEventListener("input", () => updatePercentValue(geminiOutputReservePercentInput, "gemini_max_output_tokens_percent"));
  claudeThresholdPercentInput.addEventListener("input", () => updatePercentValue(claudeThresholdPercentInput, "claude_token_threshold_percent"));
  claudeHardLimitPercentInput.addEventListener("input", () => updatePercentValue(claudeHardLimitPercentInput, "claude_max_token_limit_percent"));
  claudeOutputReservePercentInput.addEventListener("input", () => updatePercentValue(claudeOutputReservePercentInput, "claude_max_output_tokens_percent"));
  customModelThresholdPercentInput.addEventListener("input", () => updatePercentValue(customModelThresholdPercentInput, "custom_model_token_threshold_percent"));
  customModelHardLimitPercentInput.addEventListener("input", () => updatePercentValue(customModelHardLimitPercentInput, "custom_model_max_token_limit_percent"));
  customModelOutputReservePercentInput.addEventListener("input", () => updatePercentValue(customModelOutputReservePercentInput, "custom_model_max_output_tokens_percent"));

  resetButton.addEventListener("click", () => {
    if (!store.configLoaded) {
      showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
      return;
    }
    void confirmHostAction(
      t("settings.geminiCompressionResetConfirm"),
      t("settings.geminiCompressionResetConfirmTitle"),
      t("settings.geminiCompressionResetConfirmOk"),
      t("models.cancel"),
    ).then((confirmed) => {
      if (!confirmed) return;
      draftSettings = { ...DEFAULT_COMPRESSION_SETTINGS };
      render(true);
      showNotice(t("settings.geminiCompressionResetNotice"), "success");
    });
  });

  saveButton.addEventListener("click", () => {
    if (!store.configLoaded) {
      showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
      return;
    }
    if (!draftIsValid()) {
      showNotice(t("settings.geminiCompressionInvalid"), "error");
      return;
    }
    void confirmHostAction(
      t("settings.geminiCompressionSaveConfirm"),
      t("settings.geminiCompressionSaveConfirmTitle"),
      t("settings.geminiCompressionSaveConfirmOk"),
      t("models.cancel"),
    ).then((confirmed) => {
      if (!confirmed) return;
      resetButton.disabled = true;
      saveButton.disabled = true;
      void configService.saveConfig({
        ...store.config,
        official_model_settings: { ...draftSettings },
      }).then((savedConfig) => {
        store.setConfig(savedConfig);
        savedSettings = normalizeCompressionSettings(savedConfig.official_model_settings);
        draftSettings = { ...savedSettings };
        render(true);
        showNotice(t("settings.geminiCompressionSaved"), "success");
      }).catch((error: unknown) => {
        render(false);
        showNotice(t("settings.geminiCompressionSaveFailed", { message: errorMessage(error) }), "error");
      });
    });
  });

  store.subscribeConfig(syncFromStore);
  subscribeLanguage(() => render(false));
  render(true);
}
