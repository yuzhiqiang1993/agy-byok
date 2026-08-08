import { updateConfig } from "../../controllers/configController";
import { subscribeLanguage, t } from "../../i18n";
import { store } from "../../store/appStore";
import { errorMessage } from "../../utils/errorUtils";
import { confirmHostAction } from "../ConfirmModal";
import { showNotice } from "../NoticeBar";
import {
  cloneCompressionSettings,
  compressionSettingsAreEqual,
  compressionSettingsAreValid,
  DEFAULT_COMPRESSION_SETTINGS,
  updateCompressionPercentages,
  updateCompressionProfile,
  type CompressionScope,
  type PercentageField,
} from "./compressionSettingsModel";

interface CompressionControls {
  scope: CompressionScope;
  profile: HTMLSelectElement;
  parameters: HTMLElement;
  percentages: Record<PercentageField, HTMLInputElement>;
}

function createControls(scope: CompressionScope, prefix: string): CompressionControls | null {
  const profile = document.querySelector<HTMLSelectElement>(`#settings-${prefix}-compression-profile`);
  const parameters = document.querySelector<HTMLElement>(`#settings-${prefix}-custom-parameters`);
  const tokenThreshold = document.querySelector<HTMLInputElement>(`#settings-${prefix}-threshold-percent`);
  const maxTokenLimit = document.querySelector<HTMLInputElement>(`#settings-${prefix}-hard-limit-percent`);
  const maxOutputTokens = document.querySelector<HTMLInputElement>(`#settings-${prefix}-output-reserve-percent`);
  if (!profile || !parameters || !tokenThreshold || !maxTokenLimit || !maxOutputTokens) return null;
  return {
    scope,
    profile,
    parameters,
    percentages: {
      token_threshold: tokenThreshold,
      max_token_limit: maxTokenLimit,
      max_output_tokens: maxOutputTokens,
    },
  };
}

class CompressionSettingsController {
  private savedSettings = cloneCompressionSettings(store.config.official_model_settings);
  private draftSettings = cloneCompressionSettings(this.savedSettings);
  private operationInProgress = false;

  constructor(
    private readonly controls: CompressionControls[],
    private readonly resetButton: HTMLButtonElement,
    private readonly saveButton: HTMLButtonElement,
    private readonly source: HTMLElement,
  ) {}

  start(): void {
    for (const control of this.controls) {
      control.profile.addEventListener("change", () => this.changeProfile(control));
      for (const field of Object.keys(control.percentages) as PercentageField[]) {
        control.percentages[field].addEventListener("input", () => this.changePercentage(control, field));
      }
    }
    this.resetButton.addEventListener("click", () => void this.reset());
    this.saveButton.addEventListener("click", () => void this.save());
    store.subscribeConfig(() => this.syncFromStore());
    subscribeLanguage(() => this.render(false));
    this.render(true);
  }

  private render(writeValues: boolean): void {
    const configAvailable = store.configLoaded;
    for (const control of this.controls) {
      const settings = this.draftSettings[control.scope];
      const custom = settings.profile === "custom";
      control.profile.value = settings.profile;
      control.profile.disabled = !configAvailable || this.operationInProgress;
      control.parameters.hidden = !custom;
      for (const field of Object.keys(control.percentages) as PercentageField[]) {
        const input = control.percentages[field];
        input.disabled = !configAvailable || this.operationInProgress || !custom;
        if (writeValues) input.value = String(settings.percentages[field]);
      }
    }
    this.source.textContent = t("settings.compressionStrategyStatus");
    this.resetButton.disabled = this.operationInProgress
      || !configAvailable
      || compressionSettingsAreEqual(this.draftSettings, DEFAULT_COMPRESSION_SETTINGS);
    this.saveButton.disabled = this.operationInProgress
      || !configAvailable
      || compressionSettingsAreEqual(this.savedSettings, this.draftSettings)
      || !compressionSettingsAreValid(this.draftSettings);
  }

  private syncFromStore(): void {
    const incoming = cloneCompressionSettings(store.config.official_model_settings);
    if (compressionSettingsAreEqual(this.savedSettings, incoming)) {
      this.render(false);
      return;
    }
    if (!compressionSettingsAreEqual(this.savedSettings, this.draftSettings)) return;
    this.savedSettings = incoming;
    this.draftSettings = cloneCompressionSettings(incoming);
    this.render(true);
  }

  private restoreDefaultPercentages(scope: CompressionScope): void {
    this.draftSettings = updateCompressionPercentages(
      this.draftSettings,
      scope,
      { ...DEFAULT_COMPRESSION_SETTINGS[scope].percentages },
    );
  }

  private changeProfile(control: CompressionControls): void {
    const profile = control.profile.value;
    const updated = updateCompressionProfile(this.draftSettings, control.scope, profile);
    if (!updated) return;
    this.draftSettings = updated;
    if (profile !== "custom") this.restoreDefaultPercentages(control.scope);
    this.render(true);
  }

  private changePercentage(control: CompressionControls, field: PercentageField): void {
    const parsed = Number(control.percentages[field].value);
    const percentages = {
      ...this.draftSettings[control.scope].percentages,
      [field]: Number.isInteger(parsed) && parsed >= 1 && parsed <= 100 ? parsed : 0,
    };
    this.draftSettings = updateCompressionPercentages(this.draftSettings, control.scope, percentages);
    this.render(false);
  }

  private configIsAvailable(): boolean {
    if (store.configLoaded) return true;
    showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
    return false;
  }

  private async reset(): Promise<void> {
    if (this.operationInProgress || !this.configIsAvailable()) return;
    this.operationInProgress = true;
    this.render(false);
    try {
      const confirmed = await confirmHostAction(
        t("settings.geminiCompressionResetConfirm"),
        t("settings.geminiCompressionResetConfirmTitle"),
        t("settings.geminiCompressionResetConfirmOk"),
        t("models.cancel"),
      );
      if (!confirmed) return;
      this.draftSettings = cloneCompressionSettings(DEFAULT_COMPRESSION_SETTINGS);
      showNotice(t("settings.geminiCompressionResetNotice"), "success");
    } catch (error) {
      showNotice(errorMessage(error), "error");
    } finally {
      this.operationInProgress = false;
      this.render(true);
    }
  }

  private async save(): Promise<void> {
    if (this.operationInProgress || !this.configIsAvailable()) return;
    if (!compressionSettingsAreValid(this.draftSettings)) {
      showNotice(t("settings.geminiCompressionInvalid"), "error");
      return;
    }
    this.operationInProgress = true;
    this.render(false);
    try {
      const confirmed = await confirmHostAction(
        t("settings.geminiCompressionSaveConfirm"),
        t("settings.geminiCompressionSaveConfirmTitle"),
        t("settings.geminiCompressionSaveConfirmOk"),
        t("models.cancel"),
      );
      if (!confirmed) return;
      const draft = cloneCompressionSettings(this.draftSettings);
      const savedConfig = await updateConfig((current) => ({
        ...current,
        official_model_settings: draft,
      }));
      this.savedSettings = cloneCompressionSettings(savedConfig.official_model_settings);
      this.draftSettings = cloneCompressionSettings(this.savedSettings);
      showNotice(t("settings.geminiCompressionSaved"), "success");
    } catch (error) {
      showNotice(
        t("settings.geminiCompressionSaveFailed", { message: errorMessage(error) }),
        "error",
      );
    } finally {
      this.operationInProgress = false;
      this.render(true);
    }
  }
}

export function setupCompressionSettings(): void {
  const controls = [
    createControls("gemini", "gemini"),
    createControls("claude", "claude"),
    createControls("custom_model", "custom-model"),
  ];
  const resetButton = document.querySelector<HTMLButtonElement>("#reset-gemini-settings");
  const saveButton = document.querySelector<HTMLButtonElement>("#save-gemini-settings");
  const source = document.querySelector<HTMLElement>("#settings-gemini-source");
  if (controls.some((control) => !control) || !resetButton || !saveButton || !source) return;
  new CompressionSettingsController(
    controls as CompressionControls[],
    resetButton,
    saveButton,
    source,
  ).start();
}
