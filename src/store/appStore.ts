import { DEFAULT_PROXY_PORT, type AppConfig } from "../types/config";
import type { ProxyStatus } from "../types/proxy";
import type { IdeStatus, AppStatus, CliStatus } from "../types/host";

type Listener = () => void;
type ConfigLoadState = "loading" | "ready" | "error";

class AppStore {
  // Config
  private _config: AppConfig = {
    proxy_port: DEFAULT_PROXY_PORT,
    providers: [],
    upstream_models: [],
    virtual_models: [],
    official_model_settings: {
      gemini_compression_profile: "official",
      gemini_token_threshold: 640_000,
      gemini_max_token_limit: 768_000,
      gemini_max_output_tokens: 16_384,
    },
  };
  private _configLoadState: ConfigLoadState = "loading";
  private _configLoadError: string | null = null;
  
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
  
  private readonly listeners = new Set<Listener>();
  private readonly configListeners = new Set<Listener>();

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  notify(): void {
    this.listeners.forEach((listener) => listener());
  }

  subscribeConfig(listener: Listener): () => void {
    this.configListeners.add(listener);
    return () => this.configListeners.delete(listener);
  }

  // Config
  get config(): AppConfig { return this._config; }
  get configLoaded(): boolean { return this._configLoadState === "ready"; }
  get configLoadError(): string | null { return this._configLoadError; }
  setConfig(config: AppConfig): void {
    this._config = config;
    this._configLoadState = "ready";
    this._configLoadError = null;
    this.configListeners.forEach((listener) => listener());
    this.notify();
  }
  setConfigFailed(message: string): void {
    this._configLoadState = "error";
    this._configLoadError = message;
    this.configListeners.forEach((listener) => listener());
    this.notify();
  }

  // Proxy
  get proxyStatus(): ProxyStatus | null { return this._proxyStatus; }
  get proxyStatusLoadFailed(): boolean { return this._proxyStatusLoadFailed; }
  setProxyStatus(status: ProxyStatus): void {
    this._proxyStatus = status;
    this._proxyStatusLoadFailed = false;
    if (this.configLoaded && this._config.proxy_port !== status.port) {
      this._config = { ...this._config, proxy_port: status.port };
      this.configListeners.forEach((listener) => listener());
    }
    this.notify();
  }
  setProxyStatusFailed(): void { this._proxyStatus = null; this._proxyStatusLoadFailed = true; this.notify(); }

  // IDE
  get ideStatus(): IdeStatus | null { return this._ideStatus; }
  get ideStatusLoadFailed(): boolean { return this._ideStatusLoadFailed; }
  setIdeStatus(status: IdeStatus): void { this._ideStatus = status; this._ideStatusLoadFailed = false; this.notify(); }
  setIdeStatusFailed(): void { this._ideStatus = null; this._ideStatusLoadFailed = true; this.notify(); }

  // App
  get appStatus(): AppStatus | null { return this._appStatus; }
  get appStatusLoadFailed(): boolean { return this._appStatusLoadFailed; }
  setAppStatus(status: AppStatus): void { this._appStatus = status; this._appStatusLoadFailed = false; this.notify(); }
  setAppStatusFailed(): void { this._appStatus = null; this._appStatusLoadFailed = true; this.notify(); }

  // CLI
  get cliStatus(): CliStatus | null { return this._cliStatus; }
  get cliStatusLoadFailed(): boolean { return this._cliStatusLoadFailed; }
  setCliStatus(status: CliStatus): void { this._cliStatus = status; this._cliStatusLoadFailed = false; this.notify(); }
  setCliStatusFailed(): void { this._cliStatus = null; this._cliStatusLoadFailed = true; this.notify(); }


}

export const store = new AppStore();
