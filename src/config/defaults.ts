import {
  DEFAULT_PROXY_PORT,
  type AppConfig,
} from "../types/config";

export function createDefaultAppConfig(): AppConfig {
  return {
    proxy_port: DEFAULT_PROXY_PORT,
    providers: [],
    upstream_models: [],
    virtual_models: [],
    model_compression_policies: {},
    custom_host_paths: {},
  };
}
