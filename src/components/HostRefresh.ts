import { invoke } from "@tauri-apps/api/core";
import type { IdeStatus, AppStatus, CliStatus } from "../types/host";
import { renderIde } from "./IdeCard";
import { renderApp } from "./AppCard";
import { renderCli } from "./CliCard";
import { store } from "../store/appStore";
import { renderReadiness } from "./ReadinessPanel";

export async function refreshIde(): Promise<void> {
  try {
    renderIde(await invoke<IdeStatus>("discover_ide"));
  } catch (error) {
    store.setIdeStatusFailed();
    renderReadiness();
    throw error;
  }
}

export async function refreshApp(): Promise<void> {
  try {
    renderApp(await invoke<AppStatus>("discover_app"));
  } catch (error) {
    store.setAppStatusFailed();
    renderReadiness();
    throw error;
  }
}

export async function refreshCli(): Promise<void> {
  try {
    renderCli(await invoke<CliStatus>("discover_cli"));
  } catch (error) {
    store.setCliStatusFailed();
    renderReadiness();
    throw error;
  }
}

export let hostRefreshInFlight: Promise<void> | null = null;

export async function refreshHostStatuses(): Promise<void> {
  if (hostRefreshInFlight) return hostRefreshInFlight;
  const task = Promise.allSettled([refreshIde(), refreshApp(), refreshCli()]).then(() => undefined);
  hostRefreshInFlight = task;
  try {
    await task;
  } finally {
    if (hostRefreshInFlight === task) hostRefreshInFlight = null;
  }
}
