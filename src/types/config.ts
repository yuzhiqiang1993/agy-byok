import type { ReasoningLevel, ReasoningMapping } from "./reasoning";

export const DEFAULT_PROXY_PORT = 12345;

export type ProviderProtocol =
  | "openai_chat_completions"
  | "openai_responses"
  | "anthropic_messages"
  | "gemini_generate_content";

export interface ParameterOverrides {
  temperature: number | null;
  max_tokens: number | null;
  top_p: number | null;
  top_k: number | null;
  extra_body: Record<string, unknown> | null;
}

export type TokenLimitSource = "catalog" | "configured" | "estimated" | "unknown";

export interface ModelTokenLimits {
  context_window: number | null;
  context_window_source?: TokenLimitSource;
  input_token_limit: number | null;
  input_token_limit_source?: TokenLimitSource;
  output_token_limit: number | null;
  output_token_limit_source?: TokenLimitSource;
}

export type ModelCheckpointOverride =
  | {
      kind: "percentage";
      threshold_percent: number;
    }
  | {
      kind: "custom";
      token_threshold: number;
      max_token_limit: number;
      max_output_tokens: number;
    };

export type TiktokenEncoding = "cl100k_base" | "o200k_base";

export interface TokenizerConfig {
  kind: "tiktoken";
  encoding: TiktokenEncoding;
}

export type OfficialCompressionProfile =
  | "official"
  | "safe"
  | "balanced"
  | "aggressive"
  | "custom";

export type ClaudeCompressionProfile =
  | "official"
  | "safe"
  | "balanced"
  | "aggressive"
  | "custom";

export type CustomModelCompressionProfile =
  | "safe"
  | "balanced"
  | "aggressive"
  | "custom";

export interface OfficialModelSettings {
  gemini_compression_profile: OfficialCompressionProfile;
  gemini_token_threshold_percent: number;
  gemini_max_token_limit_percent: number;
  gemini_max_output_tokens_percent: number;
  claude_compression_profile: ClaudeCompressionProfile;
  claude_token_threshold_percent: number;
  claude_max_token_limit_percent: number;
  claude_max_output_tokens_percent: number;
  custom_model_compression_profile: CustomModelCompressionProfile;
  custom_model_token_threshold_percent: number;
  custom_model_max_token_limit_percent: number;
  custom_model_max_output_tokens_percent: number;
}

export interface Provider {
  id: string;
  name: string;
  protocol: ProviderProtocol;
  models_endpoint: string;
  generate_endpoint: string;
  api_key: string;
  headers: Record<string, string>;
  default_parameters: ParameterOverrides;
  connect_timeout_ms: number;
  request_timeout_ms: number;
  stream_idle_timeout_ms: number;
  enabled: boolean;
}

export interface UpstreamModel {
  id: string;
  provider_id: string;
  upstream_model_id: string;
  display_name: string;
  capabilities: {
    vision: boolean;
    tools: boolean;
    reasoning: { levels: Partial<Record<ReasoningLevel, ReasoningMapping>> };
  };
  token_limits: ModelTokenLimits;
  checkpoint_override: ModelCheckpointOverride | null;
  tokenizer: TokenizerConfig | null;
  parameter_overrides: ParameterOverrides;
  enabled: boolean;
}

export interface VirtualModel {
  id: string;
  host_model_id: string | null;
  upstream_model_id: string;
  display_name: string;
  default_reasoning_level: ReasoningLevel | null;
  parameter_overrides: ParameterOverrides;
  fallback_virtual_model_id: string | null;
  enabled: boolean;
}

export interface AppConfig {
  proxy_port: number;
  providers: Provider[];
  upstream_models: UpstreamModel[];
  virtual_models: VirtualModel[];
  official_model_settings: OfficialModelSettings;
}
