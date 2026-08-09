import type { Provider } from "../../types/config";

export const OFFICIAL_PROVIDER_ID = "official";

export const OFFICIAL_PROVIDER: Provider = {
  id: OFFICIAL_PROVIDER_ID,
  name: "🏛️ 官方原生",
  protocol: "openai_chat_completions",
  models_endpoint: "https://daily-cloudcode-pa.googleapis.com",
  generate_endpoint: "https://daily-cloudcode-pa.googleapis.com",
  api_key: "official-cloud-code-direct",
  headers: {},
  default_parameters: {
    temperature: null,
    max_tokens: null,
    top_p: null,
    top_k: null,
    extra_body: null,
  },
  connect_timeout_ms: 5000,
  request_timeout_ms: 60000,
  stream_idle_timeout_ms: 30000,
  enabled: true,
};
