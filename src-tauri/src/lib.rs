use agy_byok::domain::{
    ErrorCategory, ModelCapabilities, ParameterOverrides, Provider, ProviderProtocol, ProxyError,
    ReasoningCapability, ReasoningLevel, ReasoningMapping, UpstreamModel, VirtualModel,
};
use agy_byok::providers::{fetch_provider_models, ProviderCatalogModel};
use agy_byok::proxy::{
    ActivityItem, ActivityLog, HttpServerHandle, HttpServerOptions, LoopbackHttpServer, ProxyServer,
};
use agy_byok::storage::{default_config_path, AppConfig, ConfigStore, DEFAULT_PROXY_PORT};
use host_integration::{
    disable_ide_settings, discover, enable_ide_settings, inspect_ide_settings,
    CodeSignatureVerifier, IdeSettingsState, InstallationState, MacOsCodeSignatureVerifier,
    PatchProfile,
};
use serde::Serialize;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::State;
use tokio::sync::Mutex;

const OFFICIAL_CLOUD_CODE_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
const ANTIGRAVITY_IDE_PATH: &str = "/Applications/Antigravity IDE.app";
const ANTIGRAVITY_IDE_BUNDLE_ID: &str = "com.google.antigravity-ide";
const IDE_RESTART_TIMEOUT: Duration = Duration::from_secs(15);
const IDE_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

struct DesktopState {
    config_store: ConfigStore,
    ide_settings_path: PathBuf,
    ide_integration_root: PathBuf,
    activity_log: Arc<ActivityLog>,
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
fn save_config(mut config: AppConfig, state: State<'_, DesktopState>) -> Result<AppConfig, String> {
    // 代理端口由桌面运行时管理，必须与前端配置替换在同一写锁内合并。
    state.config_store.update_config_with(move |current| {
        config.proxy_port = current.proxy_port;
        *current = config;
    })
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
    reasoning_level: Option<ReasoningLevel>,
    custom_reasoning_value: Option<String>,
) -> Result<ModelConnectionTestResult, String> {
    let started = Instant::now();
    let config = preview_model_config(
        provider,
        upstream_model_id,
        reasoning_level,
        custom_reasoning_value.as_deref(),
    )?;
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

fn preview_reasoning_mapping(
    protocol: &ProviderProtocol,
    level: ReasoningLevel,
    custom_value: Option<&str>,
) -> Result<ReasoningMapping, String> {
    if level == ReasoningLevel::Auto {
        let value = custom_value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "自定义推理值不能为空".to_string())?;
        return match protocol {
            ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
                Ok(ReasoningMapping::Effort(value.to_string()))
            }
            ProviderProtocol::AnthropicMessages | ProviderProtocol::GeminiGenerateContent => {
                let tokens = value
                    .parse::<u32>()
                    .map_err(|_| "自定义 thinking budget 必须是整数".to_string())?;
                if tokens < 1024 {
                    return Err("自定义 thinking budget 不能小于 1024".to_string());
                }
                Ok(ReasoningMapping::BudgetTokens(tokens))
            }
        };
    }

    match protocol {
        ProviderProtocol::AnthropicMessages => Ok(ReasoningMapping::BudgetTokens(match level {
            ReasoningLevel::Low => 1024,
            ReasoningLevel::Medium => 4096,
            ReasoningLevel::High => 8192,
            ReasoningLevel::XHigh => 16384,
            ReasoningLevel::Max => 32768,
            _ => return Err("当前等级不支持 Claude 思考测试".to_string()),
        })),
        ProviderProtocol::GeminiGenerateContent => Ok(ReasoningMapping::NativeLevel(
            match level {
                ReasoningLevel::Low => "low",
                ReasoningLevel::Medium => "medium",
                ReasoningLevel::High => "high",
                _ => return Err("Gemini 只支持 Low、Medium、High 思考测试".to_string()),
            }
            .to_string(),
        )),
        ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
            Ok(ReasoningMapping::Effort(
                match level {
                    ReasoningLevel::Low => "low",
                    ReasoningLevel::Medium => "medium",
                    ReasoningLevel::High => "high",
                    ReasoningLevel::XHigh => "xhigh",
                    ReasoningLevel::Max => "max",
                    _ => return Err("当前等级不支持 OpenAI 思考测试".to_string()),
                }
                .to_string(),
            ))
        }
    }
}

fn preview_model_config(
    provider: Provider,
    upstream_model_id: String,
    reasoning_level: Option<ReasoningLevel>,
    custom_reasoning_value: Option<&str>,
) -> Result<AppConfig, String> {
    let provider_id = provider.id.clone();
    let mut reasoning = ReasoningCapability::default();
    if let Some(level) = reasoning_level {
        reasoning.levels.insert(
            level,
            preview_reasoning_mapping(&provider.protocol, level, custom_reasoning_value)?,
        );
    }
    let default_reasoning_level = reasoning_level;
    Ok(AppConfig {
        proxy_port: DEFAULT_PROXY_PORT,
        providers: vec![provider],
        upstream_models: vec![UpstreamModel {
            id: "preview-upstream".to_string(),
            provider_id,
            upstream_model_id,
            display_name: "连接预检模型".to_string(),
            capabilities: ModelCapabilities {
                reasoning,
                ..ModelCapabilities::default()
            },
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        }],
        virtual_models: vec![VirtualModel {
            id: "preview-model".to_string(),
            host_model_id: None,
            upstream_model_id: "preview-upstream".to_string(),
            display_name: "连接预检模型".to_string(),
            default_reasoning_level,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        }],
    })
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
fn get_activity_log(state: State<'_, DesktopState>) -> Vec<ActivityItem> {
    state.activity_log.get_recent()
}

#[tauri::command]
fn clear_activity_log(state: State<'_, DesktopState>) {
    state.activity_log.clear();
}

#[tauri::command]
async fn proxy_status(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await;
    Ok(status_from_handle(
        handle.as_ref(),
        state.config_store.get_config().proxy_port,
    ))
}

#[tauri::command]
async fn start_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let mut handle = state.proxy_handle.lock().await;
    if handle.is_some() {
        return Ok(status_from_handle(
            handle.as_ref(),
            state.config_store.get_config().proxy_port,
        ));
    }

    let preferred_port = state.config_store.get_config().proxy_port;
    let server = Arc::new(ProxyServer::with_activity_log(
        state.config_store.clone(),
        preferred_port,
        state.activity_log.clone(),
    ));
    let options = HttpServerOptions {
        require_auth: false,
        official_cloud_code_endpoint: Some(OFFICIAL_CLOUD_CODE_ENDPOINT.to_string()),
        fallback_to_random_port_on_bind_error: true,
        ..HttpServerOptions::default()
    };
    let started = LoopbackHttpServer::start(server, options)
        .await
        .map_err(|error| error.to_string())?;
    let actual_port = started.local_addr().port();
    if let Err(error) = state
        .config_store
        .update_config_with(|config| config.proxy_port = actual_port)
    {
        let _ = started.shutdown().await;
        return Err(format!("无法保存本地代理端口：{error}"));
    }
    *handle = Some(started);
    Ok(status_from_handle(handle.as_ref(), actual_port))
}

#[tauri::command]
async fn stop_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await.take();
    if let Some(handle) = handle {
        handle.shutdown().await.map_err(|error| error.to_string())?;
    }
    Ok(ProxyStatus {
        state: "stopped",
        address: Some(format!(
            "127.0.0.1:{}",
            state.config_store.get_config().proxy_port
        )),
    })
}

#[tauri::command]
async fn discover_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        discover_ide_sync(&settings_path, &integration_root, &endpoint)
    })
    .await
    .map_err(|error| format!("IDE discovery task failed: {error}"))?
}

#[tauri::command]
async fn enable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
        let current = discover_ide_sync(&settings_path, &integration_root, &endpoint)?;
        if current.integration_state != "disabled" {
            return Ok(current);
        }
        if !current.compatible {
            return Err(current.message);
        }
        if !current.can_enable_integration {
            return Err(current.integration_message);
        }

        let restart_ide = stop_ide_for_reconfiguration(app_path, "Antigravity IDE")?;
        if let Err(error) = enable_ide_settings(&settings_path, &integration_root, &endpoint) {
            if restart_ide {
                let _ = launch_ide_app();
            }
            return Err(error.to_string());
        }
        if restart_ide {
            restart_ide_app(app_path, "Antigravity IDE")
                .map_err(|error| format!("IDE 接入已启用，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(&settings_path, &integration_root, &endpoint)
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
    let endpoint = get_active_proxy_endpoint(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let current = discover_ide_sync(&settings_path, &integration_root, &endpoint)?;
        if !current.compatible {
            return Err(current.message);
        }
        if !current.can_launch_ide {
            return Err("请先启用 Antigravity IDE 模型注入".to_string());
        }
        launch_ide_app()
    })
    .await
    .map_err(|error| format!("IDE launch task failed: {error}"))?
}

#[tauri::command]
async fn disable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
        let current = discover_ide_sync(&settings_path, &integration_root, &endpoint)?;
        if current.integration_state == "disabled" && !current.can_disable_integration {
            return Ok(current);
        }
        if !current.can_disable_integration {
            return Err(current.integration_message);
        }

        let restart_ide = if current.ide_running {
            stop_ide_for_reconfiguration(app_path, "Antigravity IDE")?
        } else {
            false
        };
        if let Err(error) = disable_ide_settings(&settings_path, &integration_root, &endpoint) {
            if restart_ide {
                let _ = launch_ide_app();
            }
            return Err(error.to_string());
        }
        if restart_ide {
            restart_ide_app(app_path, "Antigravity IDE")
                .map_err(|error| format!("IDE 接入已停用，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(&settings_path, &integration_root, &endpoint)
    })
    .await
    .map_err(|error| format!("IDE integration deactivation task failed: {error}"))?
}

async fn get_active_proxy_endpoint(state: &DesktopState) -> String {
    let handle = state.proxy_handle.lock().await;
    let port = handle
        .as_ref()
        .map(|h| h.local_addr().port())
        .unwrap_or_else(|| state.config_store.get_config().proxy_port);
    local_proxy_endpoint(port)
}

#[tauri::command]
async fn open_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    let target = if p.exists() {
        p
    } else if let Some(parent) = p.parent() {
        if parent.exists() {
            parent
        } else {
            return Err(format!("文件及目录均不存在: {}", path));
        }
    } else {
        return Err(format!("路径不存在: {}", path));
    };

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    Ok(())
}

fn local_proxy_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn status_from_handle(handle: Option<&HttpServerHandle>, configured_port: u16) -> ProxyStatus {
    match handle {
        Some(handle) => ProxyStatus {
            state: "running",
            address: Some(handle.local_addr().to_string()),
        },
        None => ProxyStatus {
            state: "stopped",
            address: Some(format!("127.0.0.1:{configured_port}")),
        },
    }
}

fn discover_ide_sync(
    settings_path: &Path,
    integration_root: &Path,
    endpoint: &str,
) -> Result<IdeStatus, String> {
    let profile = PatchProfile::antigravity_ide_2_1_1();
    let (integration_state, integration_message, can_disable_integration, settings_valid) =
        match inspect_ide_settings(settings_path, integration_root, endpoint) {
            Ok(status) => match status.state {
                IdeSettingsState::Disabled => (
                    "disabled",
                    format!("jetski.cloudCodeUrl 尚未指向当前本地代理 {endpoint}"),
                    false,
                    true,
                ),
                IdeSettingsState::Managed if status.endpoint_matches => (
                    "enabled",
                    format!("jetski.cloudCodeUrl 已由 AGY BYOK 管理并指向当前本地代理 {endpoint}"),
                    true,
                    true,
                ),
                IdeSettingsState::Managed => (
                    "disabled",
                    format!(
                        "jetski.cloudCodeUrl 仍由 AGY BYOK 管理，但尚未指向当前本地代理 {endpoint}；可更新或停用接入"
                    ),
                    true,
                    true,
                ),
                IdeSettingsState::External => (
                    "external",
                    format!(
                        "当前相同 Endpoint {endpoint} 来自外部配置，不由 AGY BYOK 管理，无法在此停用"
                    ),
                    false,
                    true,
                ),
            },
            Err(error) => ("disabled", error.to_string(), false, false),
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

    let ide_running = is_app_running(app_path, "Antigravity IDE")?;
    let integration_message =
        if ide_running && integration_state == "disabled" && can_disable_integration {
            format!("{integration_message}；更新或停用后将自动重启 Antigravity IDE")
        } else if ide_running && integration_state == "disabled" {
            format!("{integration_message}；启用后将自动重启 Antigravity IDE")
        } else if ide_running && integration_state == "enabled" {
            format!("{integration_message}；停用后将自动重启 Antigravity IDE")
        } else {
            integration_message
        };
    let installation = match discover(app_path, &profile.layout) {
        Ok(installation) => installation,
        Err(error) => {
            return Ok(IdeStatus {
                installed: true,
                compatible: false,
                ide_running,

                state: "incompatible",
                app_path: ANTIGRAVITY_IDE_PATH.to_string(),
                app_version: None,
                extension_version: None,
                extension_sha256: None,
                message: format!("无法识别当前 Antigravity IDE 安装：{error}"),
                integration_state,
                settings_path: settings_path.display().to_string(),
                integration_message,
                can_enable_integration: false,
                can_launch_ide: false,
                can_disable_integration,
            });
        }
    };
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
    let integration_ready = integration_state != "disabled";
    let can_enable_integration = compatible && settings_valid && integration_state == "disabled";
    let can_launch_ide = compatible && integration_ready && !ide_running;

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

fn stop_ide_for_reconfiguration(app_path: &Path, label: &str) -> Result<bool, String> {
    if !is_app_running(app_path, label)? {
        return Ok(false);
    }

    let script = format!("tell application id \"{ANTIGRAVITY_IDE_BUNDLE_ID}\" to quit");
    let status = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status()
        .map_err(|error| format!("无法请求 {label} 退出：{error}"))?;
    if !status.success() {
        return Err(format!("请求 {label} 退出失败：{status}"));
    }

    wait_for_app_state(app_path, label, false)?;
    std::thread::sleep(Duration::from_millis(800));
    Ok(true)
}

fn restart_ide_app(app_path: &Path, label: &str) -> Result<(), String> {
    launch_ide_app()?;
    wait_for_app_state(app_path, label, true)
}

fn launch_ide_app() -> Result<(), String> {
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
}

fn wait_for_app_state(app_path: &Path, label: &str, expected_running: bool) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < IDE_RESTART_TIMEOUT {
        if is_app_running(app_path, label)? == expected_running {
            return Ok(());
        }
        std::thread::sleep(IDE_PROCESS_POLL_INTERVAL);
    }

    let expected = if expected_running { "启动" } else { "退出" };
    Err(format!(
        "等待 {label} {expected}超时（{} 秒）",
        IDE_RESTART_TIMEOUT.as_secs()
    ))
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
        activity_log: Arc::new(ActivityLog::new()),
        proxy_handle: Mutex::new(None),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    let state = create_state().expect("failed to initialize AGY BYOK desktop state");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            test_model_connection,
            fetch_provider_catalog,
            test_provider_model_connection,
            get_activity_log,
            clear_activity_log,
            proxy_status,
            start_proxy,
            stop_proxy,
            discover_ide,
            enable_ide_integration,
            launch_ide,
            disable_ide_integration,
            open_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGY BYOK");
}
