import type { AppStatus, CliStatus, IdeStatus } from "../types/host";
import { hostService } from "../services/hostService";
import { store } from "../store/appStore";

let ideStatusRequestVersion = 0;
let appStatusRequestVersion = 0;
let cliStatusRequestVersion = 0;
let hostRefreshInFlight: Promise<void> | null = null;

export async function refreshIde(): Promise<void> {
  const requestVersion = ++ideStatusRequestVersion;
  try {
    const status = await hostService.discoverIde();
    if (requestVersion === ideStatusRequestVersion) store.setIdeStatus(status);
  } catch (error) {
    if (requestVersion !== ideStatusRequestVersion) return;
    store.setIdeStatusFailed();
    throw error;
  }
}

export async function refreshApp(): Promise<void> {
  const requestVersion = ++appStatusRequestVersion;
  try {
    const status = await hostService.discoverApp();
    if (requestVersion === appStatusRequestVersion) store.setAppStatus(status);
  } catch (error) {
    if (requestVersion !== appStatusRequestVersion) return;
    store.setAppStatusFailed();
    throw error;
  }
}

export async function refreshCli(): Promise<void> {
  const requestVersion = ++cliStatusRequestVersion;
  try {
    const status = await hostService.discoverCli();
    if (requestVersion === cliStatusRequestVersion) store.setCliStatus(status);
  } catch (error) {
    if (requestVersion !== cliStatusRequestVersion) return;
    store.setCliStatusFailed();
    throw error;
  }
}

export async function enableIdeIntegration(): Promise<IdeStatus> {
  ideStatusRequestVersion += 1;
  const status = await hostService.enableIdeIntegration();
  ideStatusRequestVersion += 1;
  store.setIdeStatus(status);
  return status;
}

export async function disableIdeIntegration(): Promise<IdeStatus> {
  ideStatusRequestVersion += 1;
  const status = await hostService.disableIdeIntegration();
  ideStatusRequestVersion += 1;
  store.setIdeStatus(status);
  return status;
}

export async function launchIde(): Promise<void> {
  await hostService.launchIde();
}

export async function enableAppIntegration(): Promise<AppStatus> {
  appStatusRequestVersion += 1;
  try {
    const status = await hostService.enableAppIntegration();
    appStatusRequestVersion += 1;
    store.setAppStatus(status);
    return status;
  } finally {
    await refreshCli().catch(() => undefined);
  }
}

export async function disableAppIntegration(): Promise<AppStatus> {
  appStatusRequestVersion += 1;
  try {
    const status = await hostService.disableAppIntegration();
    appStatusRequestVersion += 1;
    store.setAppStatus(status);
    return status;
  } finally {
    await refreshCli().catch(() => undefined);
  }
}

export async function launchApp(): Promise<void> {
  await hostService.launchApp();
}

export async function enableCliIntegration(): Promise<CliStatus> {
  cliStatusRequestVersion += 1;
  try {
    const status = await hostService.enableCliIntegration();
    cliStatusRequestVersion += 1;
    store.setCliStatus(status);
    return status;
  } finally {
    await refreshApp().catch(() => undefined);
  }
}

export async function disableCliIntegration(): Promise<CliStatus> {
  cliStatusRequestVersion += 1;
  try {
    const status = await hostService.disableCliIntegration();
    cliStatusRequestVersion += 1;
    store.setCliStatus(status);
    return status;
  } finally {
    await refreshApp().catch(() => undefined);
  }
}

export function openPath(path: string): Promise<void> {
  return hostService.openPath(path);
}

export function openConfigDir(): Promise<void> {
  return hostService.openConfigDir();
}

export function getConfigPath(): Promise<string> {
  return hostService.getConfigPath();
}

export function openExternalUrl(url: string): Promise<void> {
  return hostService.openExternalUrl(url);
}

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
