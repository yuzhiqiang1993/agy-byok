import {
  DEFAULT_PROXY_PORT,
  type AppConfig,
  type CompressionPercentages,
  type OfficialModelSettings,
} from "../types/config";

function createDefaultPercentages(): CompressionPercentages {
  return { token_threshold: 61, max_token_limit: 73, max_output_tokens: 2 };
}

export function createDefaultOfficialModelSettings(): OfficialModelSettings {
  return {
    gemini: { profile: "official", percentages: createDefaultPercentages() },
    claude: { profile: "official", percentages: createDefaultPercentages() },
    custom_model: { profile: "balanced", percentages: createDefaultPercentages() },
  };
}

export function createDefaultAppConfig(): AppConfig {
  return {
    proxy_port: DEFAULT_PROXY_PORT,
    providers: [],
    upstream_models: [],
    virtual_models: [],
    official_model_settings: createDefaultOfficialModelSettings(),
  };
}
