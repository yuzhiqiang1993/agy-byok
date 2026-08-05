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

export interface ModelTokenLimits {
  input_token_limit: number | null;
  output_token_limit: number | null;
}

export type OfficialCompressionProfile =
  | "official"
  | "safe"
  | "balanced"
  | "aggressive"
  | "custom";

export interface OfficialModelSettings {
  gemini_compression_profile: OfficialCompressionProfile;
  gemini_token_threshold: number;
  gemini_max_token_limit: number;
  gemini_max_output_tokens: number;
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
