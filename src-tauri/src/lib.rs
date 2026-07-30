use agy_byok::domain::{
    ErrorCategory, ModelCapabilities, ParameterOverrides, Provider, ProxyError, UpstreamModel,
    VirtualModel,
};
use agy_byok::providers::{fetch_provider_models, ProviderCatalogModel};
use agy_byok::proxy::{HttpServerHandle, HttpServerOptions, LoopbackHttpServer, ProxyServer};
use agy_byok::storage::{default_config_path, AppConfig, ConfigStore};
use host_integration::{
    disable_ide_settings, discover, enable_ide_settings, inspect_ide_settings,
    CodeSignatureVerifier, IdeSettingsState, InstallationState, MacOsCodeSignatureVerifier,
    PatchProfile,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;
use tokio::sync::Mutex;

const PROXY_PORT: u16 = 50999;
const OFFICIAL_CLOUD_CODE_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
const ANTIGRAVITY_IDE_PATH: &str = "/Applications/Antigravity IDE.app";

struct DesktopState {
    config_store: ConfigStore,
    ide_settings_path: PathBuf,
    ide_integration_root: PathBuf,
    proxy_handle: Mutex<Option<HttpServerHandle>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStatus {
    state: &'static str,
    address: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelConnectionTestResult {
    success: bool,
    duration_ms: u64,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdeStatus {
    installed: bool,
    compatible: bool,
    ide_running: bool,

    state: &'static str,
    app_path: String,
    app_version: Option<String>,
    extension_version: Option<String>,
    extension_sha256: Option<String>,
    message: String,
    integration_state: &'static str,
    settings_path: String,
    integration_message: String,
    can_enable_integration: bool,
    can_launch_ide: bool,
    can_disable_integration: bool,
}

#[tauri::command]
fn get_config(state: State<'_, DesktopState>) -> AppConfig {
    state.config_store.get_config()
}

#[tauri::command]
fn save_config(config: AppConfig, state: State<'_, DesktopState>) -> Result<AppConfig, String> {
    state.config_store.update_config(config)?;
    Ok(state.config_store.get_config())
}

#[tauri::command]
async fn test_model_connection(
    virtual_model_id: String,
    state: State<'_, DesktopState>,
) -> Result<ModelConnectionTestResult, String> {
    let started = Instant::now();
    let server = ProxyServer::new(state.config_store.clone(), 0);
    let result = server.test_model_connection(&virtual_model_id).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    Ok(match result {
        Ok(()) => ModelConnectionTestResult {
            success: true,
            duration_ms,
            message: "Endpoint、鉴权、模型 ID 和响应格式均正常".to_string(),
        },
        Err(error) => ModelConnectionTestResult {
            success: false,
            duration_ms,
            message: model_connection_error_message(&error),
        },
    })
}

#[tauri::command]
async fn fetch_provider_catalog(provider: Provider) -> Result<Vec<ProviderCatalogModel>, String> {
    fetch_provider_models(&provider)
        .await
        .map_err(|error| model_connection_error_message(&error))
}

#[tauri::command]
async fn test_provider_model_connection(
    provider: Provider,
    upstream_model_id: String,
) -> Result<ModelConnectionTestResult, String> {
    let started = Instant::now();
    let config = preview_model_config(provider, upstream_model_id);
    let server = ProxyServer::new(ConfigStore::in_memory(config), 0);
    let result = server.test_model_connection("preview-model").await;
    let duration_ms = started.elapsed().as_millis() as u64;

    Ok(match result {
        Ok(()) => ModelConnectionTestResult {
            success: true,
            duration_ms,
            message: "Endpoint、鉴权、模型 ID 和响应格式均正常".to_string(),
        },
        Err(error) => ModelConnectionTestResult {
            success: false,
            duration_ms,
            message: model_connection_error_message(&error),
        },
    })
}

fn preview_model_config(provider: Provider, upstream_model_id: String) -> AppConfig {
    let provider_id = provider.id.clone();
    AppConfig {
        providers: vec![provider],
        upstream_models: vec![UpstreamModel {
            id: "preview-upstream".to_string(),
            provider_id,
            upstream_model_id,
            display_name: "连接预检模型".to_string(),
            capabilities: ModelCapabilities::default(),
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        }],
        virtual_models: vec![VirtualModel {
            id: "preview-model".to_string(),
            host_model_id: None,
            upstream_model_id: "preview-upstream".to_string(),
            display_name: "连接预检模型".to_string(),
            default_reasoning_level: None,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        }],
    }
}

/// 只向界面返回归类后的错误，避免泄露上游响应和敏感请求信息。
fn model_connection_error_message(error: &ProxyError) -> String {
    match error.category {
        ErrorCategory::Authentication => {
            format!(
                "接口已连通，但认证失败；请填写供应商要求的 API Key（HTTP {}）",
                error.status_code
            )
        }
        ErrorCategory::InvalidRequest => {
            format!("请求被上游拒绝（HTTP {}）", error.status_code)
        }
        ErrorCategory::RateLimit => {
            format!("上游正在限流（HTTP {}）", error.status_code)
        }
        ErrorCategory::ModelNotFound => {
            format!("模型不存在，请检查模型 ID（HTTP {}）", error.status_code)
        }
        ErrorCategory::UpstreamServerError => {
            format!("上游服务异常（HTTP {}）", error.status_code)
        }
        ErrorCategory::Timeout => "连接超时，15 秒内未收到完整响应".to_string(),
        ErrorCategory::ConnectionFailed => "无法连接 Endpoint，请检查地址和网络".to_string(),
        ErrorCategory::UnsupportedFeature => "当前模型配置包含不受支持的能力".to_string(),
        ErrorCategory::Internal => "上游响应格式无法识别".to_string(),
        ErrorCategory::StreamInterrupted => "上游响应意外中断".to_string(),
    }
}

#[tauri::command]
async fn proxy_status(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await;
    Ok(status_from_handle(handle.as_ref()))
}

#[tauri::command]
async fn start_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let mut handle = state.proxy_handle.lock().await;
    if handle.is_some() {
        return Ok(status_from_handle(handle.as_ref()));
    }

    let server = Arc::new(ProxyServer::new(state.config_store.clone(), PROXY_PORT));
    let options = HttpServerOptions {
        official_cloud_code_endpoint: Some(OFFICIAL_CLOUD_CODE_ENDPOINT.to_string()),
        ..HttpServerOptions::default()
    };
    let started = LoopbackHttpServer::start(server, options)
        .await
        .map_err(|error| error.to_string())?;
    *handle = Some(started);
    Ok(status_from_handle(handle.as_ref()))
}

#[tauri::command]
async fn stop_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await.take();
    if let Some(handle) = handle {
        handle.shutdown().await.map_err(|error| error.to_string())?;
    }
    Ok(ProxyStatus {
        state: "stopped",
        address: None,
    })
}

#[tauri::command]
async fn discover_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        discover_ide_sync(&settings_path, &integration_root)
    })
    .await
    .map_err(|error| format!("IDE discovery task failed: {error}"))?
}

#[tauri::command]
async fn enable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_app_not_running(Path::new(ANTIGRAVITY_IDE_PATH), "Antigravity IDE")?;
        let current = discover_ide_sync(&settings_path, &integration_root)?;
        if !current.compatible {
            return Err(current.message);
        }
        if matches!(current.integration_state, "enabled" | "external") {
            return Ok(current);
        }
        if !current.can_enable_integration {
            return Err(current.integration_message);
        }
        enable_ide_settings(&settings_path, &integration_root, local_proxy_endpoint())
            .map_err(|error| error.to_string())?;
        discover_ide_sync(&settings_path, &integration_root)
    })
    .await
    .map_err(|error| format!("IDE integration activation task failed: {error}"))?
}

#[tauri::command]
async fn launch_ide(state: State<'_, DesktopState>) -> Result<(), String> {
    if state.proxy_handle.lock().await.is_none() {
        return Err("请先启动 AGY BYOK 本地代理，再打开 Antigravity IDE".to_string());
    }
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let current = discover_ide_sync(&settings_path, &integration_root)?;
        if !current.compatible {
            return Err(current.message);
        }
        if !current.can_launch_ide {
            return Err("请先启用 Antigravity IDE 原生配置接入".to_string());
        }
        let status = Command::new("/usr/bin/open")
            .env("TMPDIR", "/private/tmp")
            .arg(ANTIGRAVITY_IDE_PATH)
            .status()
            .map_err(|error| format!("无法启动 Antigravity IDE：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("启动 Antigravity IDE 失败：{status}"))
        }
    })
    .await
    .map_err(|error| format!("IDE launch task failed: {error}"))?
}

#[tauri::command]
async fn disable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_app_not_running(Path::new(ANTIGRAVITY_IDE_PATH), "Antigravity IDE")?;
        disable_ide_settings(&settings_path, &integration_root, local_proxy_endpoint())
            .map_err(|error| error.to_string())?;
        discover_ide_sync(&settings_path, &integration_root)
    })
    .await
    .map_err(|error| format!("IDE integration deactivation task failed: {error}"))?
}

fn local_proxy_endpoint() -> &'static str {
    "http://127.0.0.1:50999"
}

fn status_from_handle(handle: Option<&HttpServerHandle>) -> ProxyStatus {
    match handle {
        Some(handle) => ProxyStatus {
            state: "running",
            address: Some(handle.local_addr().to_string()),
        },
        None => ProxyStatus {
            state: "stopped",
            address: None,
        },
    }
}

fn discover_ide_sync(settings_path: &Path, integration_root: &Path) -> Result<IdeStatus, String> {
    let profile = PatchProfile::antigravity_ide_2_1_1();
    let (integration_state, integration_message, can_disable_integration) =
        match inspect_ide_settings(settings_path, integration_root, local_proxy_endpoint()) {
            Ok(status) => match (status.state, status.receipt_path.is_some()) {
                (IdeSettingsState::Disabled, true) => (
                    "disabled",
                    "发现未完成的启用事务；可再次启用以安全继续".to_string(),
                    false,
                ),
                (IdeSettingsState::Disabled, false) => (
                    "disabled",
                    "尚未设置 jetski.cloudCodeUrl；厂商 App 保持只读".to_string(),
                    false,
                ),
                (IdeSettingsState::Enabled, _) => (
                    "enabled",
                    "AGY BYOK 已通过原生 jetski.cloudCodeUrl 接管 IDE 双 Endpoint".to_string(),
                    true,
                ),
                (IdeSettingsState::External, _) => (
                    "external",
                    "jetski.cloudCodeUrl 已由外部配置；AGY BYOK 不会覆盖或删除".to_string(),
                    false,
                ),
            },
            Err(error) => ("conflict", error.to_string(), false),
        };

    let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
    if !app_path.is_dir() {
        return Ok(IdeStatus {
            installed: false,
            compatible: false,
            ide_running: false,

            state: "not_installed",
            app_path: ANTIGRAVITY_IDE_PATH.to_string(),
            app_version: None,
            extension_version: None,
            extension_sha256: None,
            message: "未在默认位置找到厂商 Antigravity IDE".to_string(),
            integration_state,
            settings_path: settings_path.display().to_string(),
            integration_message,
            can_enable_integration: false,
            can_launch_ide: false,
            can_disable_integration,
        });
    }

    let installation = discover(app_path, &profile.layout).map_err(|error| error.to_string())?;
    let ide_running = is_app_running(app_path, "Antigravity IDE")?;
    let app_version = Some(installation.app_version.clone());
    let extension_version = Some(installation.extension_version.clone());
    let extension_sha256 = Some(installation.extension_sha256.clone());
    let (compatible, state, message) = match profile.classify(&installation) {
        Ok(InstallationState::VendorOriginal) => {
            match MacOsCodeSignatureVerifier
                .verify_vendor(&installation.app_path, &profile.bundle_id)
            {
                Ok(()) => (
                    true,
                    "vendor_original",
                    "厂商原版版本、哈希与 Google 签名匹配；不会被 AGY BYOK 修改".to_string(),
                ),
                Err(error) => (
                    false,
                    "modified",
                    format!("目标文件内容原始，但厂商签名不匹配：{error}"),
                ),
            }
        }
        Ok(InstallationState::PatchedByProfile) => (
            false,
            "patched",
            "厂商安装仍处于历史补丁状态；请重装原版后再启用配置接入".to_string(),
        ),
        Ok(InstallationState::Modified) => (
            false,
            "modified",
            "检测到未知修改，已禁止启用 IDE 配置接入".to_string(),
        ),
        Err(error) => (false, "incompatible", error.to_string()),
    };
    let integration_ready = matches!(integration_state, "enabled" | "external");
    let can_enable_integration = compatible && integration_state == "disabled" && !ide_running;
    let can_launch_ide = compatible && integration_ready && !ide_running;
    let can_disable_integration = can_disable_integration && !ide_running;
    let integration_message = if ide_running && integration_state == "disabled" {
        format!("{integration_message}；Antigravity IDE 当前正在运行，请完全退出后重新检测")
    } else if ide_running {
        format!("{integration_message}；Antigravity IDE 当前正在运行")
    } else {
        integration_message
    };

    Ok(IdeStatus {
        installed: true,
        compatible,
        ide_running,

        state,
        app_path: installation.app_path.display().to_string(),
        app_version,
        extension_version,
        extension_sha256,
        message,
        integration_state,
        settings_path: settings_path.display().to_string(),
        integration_message,
        can_enable_integration,
        can_launch_ide,
        can_disable_integration,
    })
}

fn ensure_app_not_running(app_path: &Path, label: &str) -> Result<(), String> {
    if is_app_running(app_path, label)? {
        Err(format!("请先完全退出 {label}"))
    } else {
        Ok(())
    }
}

fn is_app_running(app_path: &Path, label: &str) -> Result<bool, String> {
    let executable = app_path.join("Contents/MacOS/Electron");
    let pattern = format!("^{}( |$)", executable.display());
    let status = Command::new("pgrep")
        .args(["-f", &pattern])
        .status()
        .map_err(|error| format!("无法检查 {label} 进程：{error}"))?;
    match status.code() {
        Some(1) => Ok(false),
        Some(0) => Ok(true),
        _ => Err(format!("检查 {label} 进程失败：{status}")),
    }
}

fn create_state() -> Result<DesktopState, String> {
    let config_path = default_config_path()?;
    let config_exists = config_path.exists();
    let app_support_root = config_path
        .parent()
        .ok_or_else(|| "AGY BYOK 配置路径缺少父目录".to_string())?;
    let ide_integration_root = app_support_root.join("ide-integration");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位用户主目录，不能配置 Antigravity IDE".to_string())?;
    if !home.is_absolute() {
        return Err("用户主目录不是绝对路径，不能配置 Antigravity IDE".to_string());
    }
    let ide_settings_path =
        home.join("Library/Application Support/Antigravity IDE/User/settings.json");
    let config_store = ConfigStore::load_from_file(&config_path)?;
    if !config_exists {
        config_store.update_config(config_store.get_config())?;
    }

    Ok(DesktopState {
        config_store,
        ide_settings_path,
        ide_integration_root,
        proxy_handle: Mutex::new(None),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = create_state().expect("failed to initialize AGY BYOK desktop state");
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            test_model_connection,
            fetch_provider_catalog,
            test_provider_model_connection,
            proxy_status,
            start_proxy,
            stop_proxy,
            discover_ide,
            enable_ide_integration,
            launch_ide,
            disable_ide_integration
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGY BYOK");
}
