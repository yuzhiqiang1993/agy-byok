use crate::commands::error::{
    OFFICIAL_MODELS_FETCH_FAILED, OFFICIAL_MODELS_HOST_NOT_INSTALLED,
    OFFICIAL_MODELS_HOST_NOT_RUNNING, OFFICIAL_MODELS_PROXY_REQUIRED, PROVIDER_CATALOG_FAILED,
};
use crate::host::app_host::{discover_app_sync, AppStatus};
use crate::host::cli_host::discover_cli_sync;
use crate::host::cli_host::CliStatus;
use crate::host::ide_host::{discover_ide_sync, IdeStatus};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use agy_byok::domain::{
    AppConfig, ModelCapabilities, ModelTokenLimits, ParameterOverrides, Provider, ProviderProtocol,
    ProxyError, ReasoningCapability, ReasoningLevel, ReasoningMapping, UpstreamModel, VirtualModel,
    DEFAULT_PROXY_PORT,
};
use agy_byok::providers::{
    fetch_official_models_catalog, fetch_official_models_catalog_raw, fetch_provider_models,
    fetch_provider_models_raw, parse_official_catalog_response, OfficialCatalogRawResponse,
    OfficialCatalogSource, ProviderCatalogModel,
};
use agy_byok::proxy::ProxyServer;
use agy_byok::storage::ConfigStore;
use host_integration::detect_cli_executable;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::State;

const RUNNING_HOST_RETRY_TIMEOUT: Duration = Duration::from_secs(4);
const OFFICIAL_MODELS_RETRY_INTERVAL: Duration = Duration::from_millis(400);
const CLI_MODELS_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionTestResult {
    pub success: bool,
    pub duration_ms: u64,
    pub error_category: Option<&'static str>,
    pub status_code: Option<u16>,
}

#[tauri::command]
pub(crate) async fn test_model_connection(
    virtual_model_id: String,
    state: State<'_, DesktopState>,
) -> Result<ModelConnectionTestResult, String> {
    let started = Instant::now();
    let server = ProxyServer::new(state.config_store.clone(), 0);
    let result = server.test_model_connection(&virtual_model_id).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    Ok(connection_test_result(result, duration_ms))
}

#[tauri::command]
pub(crate) async fn fetch_provider_catalog(
    provider: Provider,
) -> Result<Vec<ProviderCatalogModel>, String> {
    fetch_provider_models(&provider).await.map_err(|error| {
        tracing::warn!(error = %error, "获取供应商模型目录失败");
        PROVIDER_CATALOG_FAILED.to_string()
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogDebugResult {
    pub success: bool,
    pub request_url: String,
    pub status_code: Option<u16>,
    pub content_type: Option<String>,
    pub error_category: Option<&'static str>,
    pub error_message: Option<String>,
    pub response_body: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialModelsDebugResult {
    pub success: bool,
    pub source: Option<String>,
    pub request_url: Option<String>,
    pub status_code: Option<u16>,
    pub content_type: Option<String>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub raw_response: Option<String>,
    pub normalized_models: Vec<ProviderCatalogModel>,
}

#[tauri::command]
pub(crate) async fn fetch_provider_catalog_debug(
    provider: Provider,
) -> Result<ProviderCatalogDebugResult, String> {
    if !cfg!(debug_assertions) {
        return Err("provider_catalog_debug_disabled".to_string());
    }

    Ok(match fetch_provider_models_raw(&provider).await {
        Ok(raw) => ProviderCatalogDebugResult {
            success: raw.status_code < 400,
            request_url: raw.request_url,
            status_code: Some(raw.status_code),
            content_type: raw.content_type,
            error_category: None,
            error_message: (raw.status_code >= 400)
                .then(|| format!("模型目录返回 HTTP {}", raw.status_code)),
            response_body: raw.body,
        },
        Err(error) => ProviderCatalogDebugResult {
            success: false,
            request_url: provider.models_endpoint,
            status_code: None,
            content_type: None,
            error_category: Some(error.category.as_str()),
            error_message: Some(error.message),
            response_body: error.upstream_body.unwrap_or_default(),
        },
    })
}

#[derive(Debug, Deserialize)]
struct CliModelsOutput {
    command: CliModelsCommand,
}

#[derive(Debug, Deserialize)]
struct CliModelsCommand {
    data: CliModelsData,
}

#[derive(Debug, Deserialize)]
struct CliModelsData {
    models: Vec<CliModel>,
}

#[derive(Debug, Deserialize)]
struct CliModel {
    id: String,
    label: String,
}

async fn fetch_desktop_official_models(
    source: OfficialCatalogSource,
    retry_timeout: Duration,
) -> Result<Vec<ProviderCatalogModel>, ProxyError> {
    let deadline = Instant::now() + retry_timeout;
    loop {
        match fetch_official_models_catalog(source).await {
            Ok(models) => return Ok(models),
            Err(error) if matches!(error.status_code, 404 | 502) && Instant::now() < deadline => {
                tokio::time::sleep(OFFICIAL_MODELS_RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn fetch_desktop_official_models_raw(
    source: OfficialCatalogSource,
    retry_timeout: Duration,
) -> Result<OfficialCatalogRawResponse, ProxyError> {
    let deadline = Instant::now() + retry_timeout;
    loop {
        match fetch_official_models_catalog_raw(source).await {
            Ok(raw) => return Ok(raw),
            Err(error) if matches!(error.status_code, 404 | 502) && Instant::now() < deadline => {
                tokio::time::sleep(OFFICIAL_MODELS_RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn parse_cli_models_output(stdout: &str) -> Result<Vec<ProviderCatalogModel>, String> {
    let json_line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| "CLI 未返回模型目录 JSON".to_string())?;
    let output: CliModelsOutput = serde_json::from_str(json_line)
        .map_err(|error| format!("解析 CLI 模型目录失败：{error}"))?;
    let mut models = Vec::new();
    for model in output.command.data.models {
        let id = model.id.trim();
        if id.is_empty()
            || models
                .iter()
                .any(|existing: &ProviderCatalogModel| existing.id == id)
        {
            continue;
        }
        models.push(ProviderCatalogModel {
            id: id.to_string(),
            display_name: model.label.trim().to_string(),
            ..ProviderCatalogModel::default()
        });
    }
    (!models.is_empty())
        .then_some(models)
        .ok_or_else(|| "CLI 返回的模型目录为空".to_string())
}

async fn fetch_cli_official_models() -> Result<Vec<ProviderCatalogModel>, String> {
    let raw = fetch_cli_official_models_raw().await?;
    parse_cli_models_output(&raw)
}

async fn fetch_cli_official_models_raw() -> Result<String, String> {
    let executable =
        detect_cli_executable().ok_or_else(|| "无法定位 CLI 可执行文件".to_string())?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["--output-format", "json", "models"])
        .kill_on_drop(true);
    let output = tokio::time::timeout(CLI_MODELS_TIMEOUT, command.output())
        .await
        .map_err(|_| "CLI 获取模型目录超时".to_string())?
        .map_err(|error| format!("启动 CLI 获取模型目录失败：{error}"))?;
    if !output.status.success() {
        return Err(format!("CLI 获取模型目录失败：{}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn discover_official_host_statuses(
    state: &DesktopState,
) -> Result<(IdeStatus, AppStatus, CliStatus), String> {
    let snapshot = proxy_runtime_snapshot(state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let host_paths = state.current_host_paths();
    let ide_paths = host_paths.ide;
    let app_paths = host_paths.app;
    let integration_root = state.host_integration_root.clone();
    let status_endpoint = endpoint.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, String>((
            discover_ide_sync(
                ide_paths.as_ref(),
                &integration_root,
                &status_endpoint,
                proxy_running,
            )?,
            discover_app_sync(
                app_paths.as_ref(),
                &integration_root,
                &status_endpoint,
                proxy_running,
            )?,
            discover_cli_sync(&integration_root, &status_endpoint, proxy_running)?,
        ))
    })
    .await
    .map_err(|error| {
        tracing::warn!(%error, "检测官方模型来源失败");
        OFFICIAL_MODELS_FETCH_FAILED.to_string()
    })?
    .map_err(|error| {
        tracing::warn!(%error, "检测官方模型来源失败");
        OFFICIAL_MODELS_FETCH_FAILED.to_string()
    })
}

#[tauri::command]
pub(crate) async fn fetch_official_models(
    state: State<'_, DesktopState>,
) -> Result<Vec<ProviderCatalogModel>, String> {
    let statuses = discover_official_host_statuses(&state).await?;
    let proxy_running = proxy_runtime_snapshot(&state).await.running;
    let (ide_status, app_status, cli_status) = statuses;
    if !ide_status.installed && !app_status.installed && !cli_status.installed {
        return Err(OFFICIAL_MODELS_HOST_NOT_INSTALLED.to_string());
    }

    let mut found_stopped_host = false;
    let mut proxy_required = false;

    if ide_status.installed {
        if !ide_status.ide_running {
            found_stopped_host = true;
        } else {
            match fetch_desktop_official_models(
                OfficialCatalogSource::Ide,
                RUNNING_HOST_RETRY_TIMEOUT,
            )
            .await
            {
                Ok(models) => return Ok(models),
                Err(error) => tracing::warn!(%error, "通过 IDE 获取官方模型失败，尝试下一来源"),
            }
        }
    }

    if app_status.installed {
        if !app_status.app_running {
            found_stopped_host = true;
        } else {
            match fetch_desktop_official_models(
                OfficialCatalogSource::App,
                RUNNING_HOST_RETRY_TIMEOUT,
            )
            .await
            {
                Ok(models) => return Ok(models),
                Err(error) => tracing::warn!(%error, "通过 App 获取官方模型失败，尝试下一来源"),
            }
        }
    }

    if cli_status.installed {
        match fetch_cli_official_models().await {
            Ok(models) => return Ok(models),
            Err(error) => {
                if cli_status.integration_state.is_ready() && !proxy_running {
                    proxy_required = true;
                }
                tracing::warn!(%error, "通过 CLI 获取官方模型失败");
            }
        }
    }

    if proxy_required {
        return Err(OFFICIAL_MODELS_PROXY_REQUIRED.to_string());
    }
    if found_stopped_host {
        return Err(OFFICIAL_MODELS_HOST_NOT_RUNNING.to_string());
    }
    Err(OFFICIAL_MODELS_FETCH_FAILED.to_string())
}

fn official_debug_failure(
    category: impl Into<String>,
    message: impl Into<String>,
    raw_response: Option<String>,
) -> OfficialModelsDebugResult {
    OfficialModelsDebugResult {
        success: false,
        source: None,
        request_url: None,
        status_code: None,
        content_type: None,
        error_category: Some(category.into()),
        error_message: Some(message.into()),
        raw_response,
        normalized_models: Vec::new(),
    }
}

fn official_debug_success(
    raw: OfficialCatalogRawResponse,
    models: Vec<ProviderCatalogModel>,
) -> OfficialModelsDebugResult {
    OfficialModelsDebugResult {
        success: true,
        source: Some(raw.source),
        request_url: Some(raw.request_url),
        status_code: Some(raw.status_code),
        content_type: raw.content_type,
        error_category: None,
        error_message: None,
        raw_response: Some(raw.body),
        normalized_models: models,
    }
}

#[tauri::command]
pub(crate) async fn fetch_official_models_debug(
    state: State<'_, DesktopState>,
) -> Result<OfficialModelsDebugResult, String> {
    if !cfg!(debug_assertions) {
        return Err("official_models_debug_disabled".to_string());
    }

    let statuses = discover_official_host_statuses(&state).await?;
    let (ide_status, app_status, cli_status) = statuses;
    if !ide_status.installed && !app_status.installed && !cli_status.installed {
        return Ok(official_debug_failure(
            "official_models_host_not_installed",
            "未检测到 Antigravity IDE、App 或 CLI",
            None,
        ));
    }

    let mut found_stopped_host = false;
    let mut last_error: Option<OfficialModelsDebugResult> = None;
    for (installed, running, source) in [
        (ide_status.installed, ide_status.ide_running, OfficialCatalogSource::Ide),
        (app_status.installed, app_status.app_running, OfficialCatalogSource::App),
    ] {
        if !installed {
            continue;
        }
        if !running {
            found_stopped_host = true;
            continue;
        }

        match fetch_desktop_official_models_raw(source, RUNNING_HOST_RETRY_TIMEOUT).await {
            Ok(raw) => match parse_official_catalog_response(&raw.body, raw.source.as_str()) {
                Ok(models) => return Ok(official_debug_success(raw, models)),
                Err(error) => {
                    last_error = Some(official_debug_failure(
                        error.category.as_str(),
                        error.message,
                        error.upstream_body,
                    ));
                }
            },
            Err(error) => {
                last_error = Some(official_debug_failure(
                    error.category.as_str(),
                    error.message,
                    error.upstream_body,
                ));
            }
        }
    }

    if cli_status.installed {
        match fetch_cli_official_models_raw().await {
            Ok(raw) => match parse_cli_models_output(&raw) {
                Ok(models) => {
                    return Ok(OfficialModelsDebugResult {
                        success: true,
                        source: Some("Antigravity CLI".to_string()),
                        request_url: Some("agy --output-format json models".to_string()),
                        status_code: None,
                        content_type: Some("application/json".to_string()),
                        error_category: None,
                        error_message: None,
                        raw_response: Some(raw),
                        normalized_models: models,
                    });
                }
                Err(error) => {
                    last_error = Some(official_debug_failure(
                        "upstream_server_error",
                        error,
                        Some(raw),
                    ));
                }
            },
            Err(error) => {
                last_error = Some(official_debug_failure("upstream_server_error", error, None));
            }
        }
    }

    Ok(last_error.unwrap_or_else(|| {
        if found_stopped_host {
            official_debug_failure(
                "official_models_host_not_running",
                "已安装 Antigravity 客户端，但当前未运行",
                None,
            )
        } else {
            official_debug_failure(
                "official_models_fetch_failed",
                "所有官方模型来源均获取失败",
                None,
            )
        }
    }))
}

#[tauri::command]
pub(crate) async fn test_provider_model_connection(
    provider: Provider,
    upstream_model_id: String,
    reasoning_level: Option<ReasoningLevel>,
    custom_reasoning_value: Option<String>,
    reasoning_mapping: Option<ReasoningMapping>,
) -> ModelConnectionTestResult {
    let started = Instant::now();
    let config = match preview_model_config(
        provider,
        upstream_model_id,
        reasoning_level,
        custom_reasoning_value.as_deref(),
        reasoning_mapping.as_ref(),
    ) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(%error, "模型连接预检配置无效");
            return ModelConnectionTestResult {
                success: false,
                duration_ms: started.elapsed().as_millis() as u64,
                error_category: Some("invalid_configuration"),
                status_code: None,
            };
        }
    };
    let server = ProxyServer::new(ConfigStore::in_memory(config), 0);
    let result = match reasoning_level {
        Some(level) => {
            server
                .test_model_connection_with_reasoning("preview-model", level)
                .await
        }
        None => server.test_model_connection("preview-model").await,
    };
    let duration_ms = started.elapsed().as_millis() as u64;

    connection_test_result(result, duration_ms)
}

fn connection_test_result(
    result: Result<(), ProxyError>,
    duration_ms: u64,
) -> ModelConnectionTestResult {
    match result {
        Ok(()) => ModelConnectionTestResult {
            success: true,
            duration_ms,
            error_category: None,
            status_code: None,
        },
        Err(error) => ModelConnectionTestResult {
            success: false,
            duration_ms,
            error_category: Some(error.category.as_str()),
            status_code: (error.status_code > 0).then_some(error.status_code),
        },
    }
}

fn preview_reasoning_mapping(
    protocol: &ProviderProtocol,
    level: ReasoningLevel,
    custom_value: Option<&str>,
    catalog_mapping: Option<&ReasoningMapping>,
) -> Result<ReasoningMapping, String> {
    if let Some(mapping) = catalog_mapping {
        return Ok(mapping.clone());
    }
    if level == ReasoningLevel::Auto {
        let value = custom_value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "自定义推理值不能为空".to_string())?;
        return match protocol {
            ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
                Ok(ReasoningMapping::Effort(value.to_string()))
            }
            ProviderProtocol::AnthropicMessages => {
                if let Ok(tokens) = value.parse::<u32>() {
                    if tokens < 1024 {
                        return Err("自定义 thinking budget 不能小于 1024".to_string());
                    }
                    Ok(ReasoningMapping::BudgetTokens(tokens))
                } else {
                    Ok(ReasoningMapping::Effort(value.to_string()))
                }
            }
            ProviderProtocol::GeminiGenerateContent => {
                if let Ok(tokens) = value.parse::<u32>() {
                    if tokens < 1024 {
                        return Err("自定义 thinking budget 不能小于 1024".to_string());
                    }
                    Ok(ReasoningMapping::BudgetTokens(tokens))
                } else {
                    Ok(ReasoningMapping::NativeLevel(value.to_string()))
                }
            }
        };
    }

    Err("模型目录没有提供当前思考等级的可用映射，请重新获取模型目录".to_string())
}

fn preview_model_config(
    provider: Provider,
    upstream_model_id: String,
    reasoning_level: Option<ReasoningLevel>,
    custom_reasoning_value: Option<&str>,
    catalog_mapping: Option<&ReasoningMapping>,
) -> Result<AppConfig, String> {
    let provider_id = provider.id.clone();
    let mut reasoning = ReasoningCapability::default();
    if let Some(level) = reasoning_level {
        reasoning.levels.insert(
            level,
            preview_reasoning_mapping(
                &provider.protocol,
                level,
                custom_reasoning_value,
                catalog_mapping,
            )?,
        );
    }
    let default_reasoning_level = reasoning_level;
    let config = AppConfig {
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
            token_limits: ModelTokenLimits::default(),
            compression_policy: None,
            tokenizer: None,
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
        model_compression_policies: Default::default(),
        custom_host_paths: Default::default(),
    };
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview_provider() -> Provider {
        Provider {
            id: "preview-provider".to_string(),
            name: "Preview Provider".to_string(),
            protocol: ProviderProtocol::OpenaiChatCompletions,
            models_endpoint: "https://example.com/v1/models".to_string(),
            generate_endpoint: "https://example.com/v1/chat/completions".to_string(),
            api_key: String::new(),
            headers: Default::default(),
            default_parameters: ParameterOverrides::default(),
            connect_timeout_ms: 5_000,
            request_timeout_ms: 15_000,
            stream_idle_timeout_ms: 30_000,
            enabled: true,
        }
    }

    #[test]
    fn preview_model_config_rejects_invalid_provider_timeouts() {
        let mut provider = preview_provider();
        provider.request_timeout_ms = 0;

        assert!(preview_model_config(provider, "model".to_string(), None, None, None).is_err());
    }

    #[test]
    fn cli_model_output_is_parsed_and_deduplicated() {
        let output = r#"{"command":{"data":{"models":[{"id":"gemini-3.6-flash-high","label":"Gemini 3.6 Flash High"},{"id":"gemini-3.6-flash-high","label":"Duplicate"},{"id":"claude-sonnet-4-6","label":"Claude Sonnet 4.6"}]}}}"#;

        let models = parse_cli_models_output(output).unwrap();

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gemini-3.6-flash-high");
        assert_eq!(models[0].display_name, "Gemini 3.6 Flash High");
        assert_eq!(models[0].is_agent_model, None);
        assert_eq!(models[0].agent_sort_order, None);
        assert_eq!(models[1].id, "claude-sonnet-4-6");
    }
}
