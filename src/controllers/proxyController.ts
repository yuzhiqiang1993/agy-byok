import type { ProxyStatus } from "../types/proxy";
import { proxyService } from "../services/proxyService";
import { store } from "../store/appStore";

export async function refreshProxy(): Promise<ProxyStatus> {
  try {
    const status = await proxyService.getStatus();
    store.setProxyStatus(status);
    return status;
  } catch (error) {
    store.setProxyStatusFailed();
    throw error;
  }
}

export async function startProxy(): Promise<ProxyStatus> {
  const status = await proxyService.start();
  store.setProxyStatus(status);
  return status;
}

export async function stopProxy(): Promise<ProxyStatus> {
  const status = await proxyService.stop();
  store.setProxyStatus(status);
  return status;
}

export async function setProxyPort(port: number): Promise<ProxyStatus> {
  const status = await proxyService.setPort(port);
  store.setProxyStatus(status);
  return status;
}
