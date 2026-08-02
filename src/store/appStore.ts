import type { AppConfig } from "../types/config";
import type { ProxyStatus, ConnectionTestViewState, ProviderTestSession } from "../types/proxy";
import type { IdeStatus, AppStatus, CliStatus } from "../types/host";
import type { ActivityItem } from "../types/activity";

type Listener = () => void;

class AppStore {
  // Config
  private _config: AppConfig = { proxy_port: 54321, providers: [], upstream_models: [], virtual_models: [] };
  
  // Proxy
  private _proxyStatus: ProxyStatus | null = null;
  private _proxyStatusLoadFailed = false;
  
  // Host statuses
  private _ideStatus: IdeStatus | null = null;
  private _ideStatusLoadFailed = false;
  private _appStatus: AppStatus | null = null;
  private _appStatusLoadFailed = false;
  private _cliStatus: CliStatus | null = null;
  private _cliStatusLoadFailed = false;
  
  // Activity
  private _activityItems: ActivityItem[] = [];
  private _activityFailedOnly = false;
  private _activitySnapshot = "";
  private _activityActionInProgress = false;

  // Connection tests
  readonly connectionTestResults = new Map<string, ConnectionTestViewState>();
  readonly providerTestSessions = new Map<string, ProviderTestSession>();
  
  // Misc editing state
  private _editingProviderId: string | null = null;
  private _activeProviderTabId: string | null = null;
  private _configMutationInProgress = false;
  private _noticeTimer: number | null = null;

  private readonly listeners = new Set<Listener>();

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  notify(): void {
    this.listeners.forEach(l => l());
  }

  // Config
  get config(): AppConfig { return this._config; }
  setConfig(config: AppConfig): void { this._config = config; this.notify(); }

  // Proxy
  get proxyStatus(): ProxyStatus | null { return this._proxyStatus; }
  get proxyStatusLoadFailed(): boolean { return this._proxyStatusLoadFailed; }
  setProxyStatus(status: ProxyStatus): void { this._proxyStatus = status; this._proxyStatusLoadFailed = false; this.notify(); }
  setProxyStatusFailed(): void { this._proxyStatusLoadFailed = true; this.notify(); }

  // IDE
  get ideStatus(): IdeStatus | null { return this._ideStatus; }
  get ideStatusLoadFailed(): boolean { return this._ideStatusLoadFailed; }
  setIdeStatus(status: IdeStatus): void { this._ideStatus = status; this._ideStatusLoadFailed = false; this.notify(); }
  setIdeStatusFailed(): void { this._ideStatusLoadFailed = true; this.notify(); }

  // App
  get appStatus(): AppStatus | null { return this._appStatus; }
  get appStatusLoadFailed(): boolean { return this._appStatusLoadFailed; }
  setAppStatus(status: AppStatus): void { this._appStatus = status; this._appStatusLoadFailed = false; this.notify(); }
  setAppStatusFailed(): void { this._appStatusLoadFailed = true; this.notify(); }

  // CLI
  get cliStatus(): CliStatus | null { return this._cliStatus; }
  get cliStatusLoadFailed(): boolean { return this._cliStatusLoadFailed; }
  setCliStatus(status: CliStatus): void { this._cliStatus = status; this._cliStatusLoadFailed = false; this.notify(); }
  setCliStatusFailed(): void { this._cliStatusLoadFailed = true; this.notify(); }

  // Activity
  get activityItems(): ActivityItem[] { return this._activityItems; }
  setActivityItems(items: ActivityItem[]): void { this._activityItems = items; this.notify(); }
  get activityFailedOnly(): boolean { return this._activityFailedOnly; }
  setActivityFailedOnly(v: boolean): void { this._activityFailedOnly = v; }
  get activitySnapshot(): string { return this._activitySnapshot; }
  setActivitySnapshot(v: string): void { this._activitySnapshot = v; }
  get activityActionInProgress(): boolean { return this._activityActionInProgress; }
  setActivityActionInProgress(v: boolean): void { this._activityActionInProgress = v; }

  // Editing state
  get editingProviderId(): string | null { return this._editingProviderId; }
  setEditingProviderId(id: string | null): void { this._editingProviderId = id; }
  get activeProviderTabId(): string | null { return this._activeProviderTabId; }
  setActiveProviderTabId(id: string | null): void { this._activeProviderTabId = id; this.notify(); }
  get configMutationInProgress(): boolean { return this._configMutationInProgress; }
  setConfigMutationInProgress(v: boolean): void { this._configMutationInProgress = v; }
  get noticeTimer(): number | null { return this._noticeTimer; }
  setNoticeTimer(timer: number | null): void { this._noticeTimer = timer; }
}

export const store = new AppStore();
