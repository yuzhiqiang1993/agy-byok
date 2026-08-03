import type { ModelConnectionTestOutcome, ConnectionTestViewState, ProviderTestSession } from "../../types/proxy";

let activeProviderTabId: string | null = null;
let providerEditorDirty = false;

export const connectionTestsInFlight = new Map<string, Promise<ModelConnectionTestOutcome>>();
export const connectionTestResults = new Map<string, ConnectionTestViewState>();
export const providerTestSessions = new Map<string, ProviderTestSession>();

export function getActiveProviderTabId(): string | null {
  return activeProviderTabId;
}

export function setActiveProviderTabId(id: string | null): void {
  activeProviderTabId = id;
}

export function isProviderEditorDirty(): boolean {
  return providerEditorDirty;
}

export function setProviderEditorDirtyState(dirty: boolean): void {
  providerEditorDirty = dirty;
}
