import { invoke } from "@tauri-apps/api/core";
import type { ProxyStatus } from "../types/proxy";

export const proxyService = {
  getStatus: () => invoke<ProxyStatus>("proxy_status"),
  start: () => invoke<ProxyStatus>("start_proxy"),
  stop: () => invoke<ProxyStatus>("stop_proxy"),
  setPort: (port: number) => invoke<ProxyStatus>("set_proxy_port", { port }),
};
