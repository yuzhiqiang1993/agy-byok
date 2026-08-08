import { createDefaultOfficialModelSettings } from "../../config/defaults";
import type {
  CompressionPercentages,
  CustomModelCompressionProfile,
  OfficialCompressionProfile,
  OfficialModelSettings,
} from "../../types/config";

export type CompressionScope = keyof OfficialModelSettings;
export type PercentageField = keyof CompressionPercentages;

export const DEFAULT_COMPRESSION_SETTINGS = createDefaultOfficialModelSettings();

export function cloneCompressionSettings(value: OfficialModelSettings): OfficialModelSettings {
  return {
    gemini: { profile: value.gemini.profile, percentages: { ...value.gemini.percentages } },
    claude: { profile: value.claude.profile, percentages: { ...value.claude.percentages } },
    custom_model: {
      profile: value.custom_model.profile,
      percentages: { ...value.custom_model.percentages },
    },
  };
}

function isOfficialProfile(value: string): value is OfficialCompressionProfile {
  return ["official", "safe", "balanced", "aggressive", "custom"].includes(value);
}

function isCustomModelProfile(value: string): value is CustomModelCompressionProfile {
  return ["none", "safe", "balanced", "aggressive", "custom"].includes(value);
}

export function percentagesAreValid(value: CompressionPercentages): boolean {
  const { token_threshold, max_token_limit, max_output_tokens } = value;
  return Number.isInteger(token_threshold)
    && Number.isInteger(max_token_limit)
    && Number.isInteger(max_output_tokens)
    && token_threshold >= 1
    && max_token_limit >= 1
    && max_output_tokens >= 1
    && token_threshold <= 100
    && max_token_limit <= 100
    && max_output_tokens <= 100
    && token_threshold < max_token_limit
    && max_output_tokens < max_token_limit
    && token_threshold + max_output_tokens <= max_token_limit;
}

export function compressionSettingsAreValid(value: OfficialModelSettings): boolean {
  return percentagesAreValid(value.gemini.percentages)
    && percentagesAreValid(value.claude.percentages)
    && percentagesAreValid(value.custom_model.percentages);
}

export function compressionSettingsAreEqual(
  left: OfficialModelSettings,
  right: OfficialModelSettings,
): boolean {
  return (Object.keys(left) as CompressionScope[]).every((scope) => {
    const leftGroup = left[scope];
    const rightGroup = right[scope];
    return leftGroup.profile === rightGroup.profile
      && leftGroup.percentages.token_threshold === rightGroup.percentages.token_threshold
      && leftGroup.percentages.max_token_limit === rightGroup.percentages.max_token_limit
      && leftGroup.percentages.max_output_tokens === rightGroup.percentages.max_output_tokens;
  });
}

export function updateCompressionPercentages(
  settings: OfficialModelSettings,
  scope: CompressionScope,
  percentages: CompressionPercentages,
): OfficialModelSettings {
  if (scope === "gemini") return { ...settings, gemini: { ...settings.gemini, percentages } };
  if (scope === "claude") return { ...settings, claude: { ...settings.claude, percentages } };
  return { ...settings, custom_model: { ...settings.custom_model, percentages } };
}

export function updateCompressionProfile(
  settings: OfficialModelSettings,
  scope: CompressionScope,
  profile: string,
): OfficialModelSettings | null {
  if (scope === "custom_model") {
    if (!isCustomModelProfile(profile)) return null;
    return { ...settings, custom_model: { ...settings.custom_model, profile } };
  }
  if (!isOfficialProfile(profile)) return null;
  if (scope === "gemini") return { ...settings, gemini: { ...settings.gemini, profile } };
  return { ...settings, claude: { ...settings.claude, profile } };
}
