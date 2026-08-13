use crate::commands::error::{
    OFFICIAL_MODELS_FETCH_FAILED, OFFICIAL_MODELS_HOST_NOT_INSTALLED,
    OFFICIAL_MODELS_HOST_NOT_RUNNING, PROVIDER_CATALOG_FAILED,
};
use crate::host::app_host::{discover_app_sync, AppStatus};

use crate::host::ide_host::{discover_ide_sync, IdeStatus};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use agy_byok::domain::{
    AppConfig, ModelCapabilities, ModelCompressionPolicy, ModelModality, ModelRole,
    ModelTokenLimits, ParameterOverrides, Provider, ProviderProtocol, ProxyError,
    ReasoningCapability, ReasoningLevel, ReasoningMapping, UpstreamModel, VirtualModel,
    DEFAULT_PROXY_PORT,
};
use agy_byok::providers::{
    fetch_official_models_catalog, fetch_official_models_catalog_raw, fetch_provider_models,
    fetch_provider_models_raw, parse_official_catalog_response, OfficialCatalogRawResponse,
    OfficialCatalogSource, ProviderCatalogModel,
};
use agy_byok::proxy::ProxyServer;
use agy_byok::storage::ConfigStore;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::State;

const RUNNING_HOST_RETRY_TIMEOUT: Duration = Duration::from_secs(4);
const OFFICIAL_MODELS_RETRY_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionTestResult {
    pub success: bool,
    pub duration_ms: u64,
    pub error_category: Option<&'static str>,
    pub status_code: Option<u16>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub error_message: Option<String>,
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
pub(crate) fn resolve_effective_compression_policy(
    policy: ModelCompressionPolicy,
    capacity: Option<u32>,
    output_token_limit: Option<u32>,
) -> Result<ModelCompressionPolicy, String> {
    policy
        .resolve_effective(capacity, output_token_limit)
        .ok_or_else(|| "compression_policy_cannot_be_resolved".to_string())
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
    pub modified_response: Option<String>,
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

async fn discover_official_host_statuses(
    state: &DesktopState,
) -> Result<(IdeStatus, AppStatus), String> {
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
    let (ide_status, app_status) = statuses;
    if !ide_status.installed && !app_status.installed {
        return Err(OFFICIAL_MODELS_HOST_NOT_INSTALLED.to_string());
    }

    let mut found_stopped_host = false;

    // Trigger a request to the language server to ensure the proxy caches the raw upstream response.
    // The language server might return a filtered response, so we ignore its body and read the proxy cache.
    let mut request_succeeded = false;
    for (installed, running, source) in [
        (
            ide_status.installed,
            ide_status.ide_running,
            OfficialCatalogSource::Ide,
        ),
        (
            app_status.installed,
            app_status.app_running,
            OfficialCatalogSource::App,
        ),
    ] {
        if !installed {
            continue;
        }
        if !running {
            found_stopped_host = true;
            continue;
        }
        match fetch_desktop_official_models_raw(source, RUNNING_HOST_RETRY_TIMEOUT).await {
            Ok(_) => {
                request_succeeded = true;
                break;
            }
            Err(error) => {
                tracing::warn!(%error, "通过 {} 获取官方模型失败，尝试下一来源", source.label());
            }
        }
    }

    if let Some(raw_catalog) = state.config_store.get_raw_official_catalog() {
        if let Ok(models) = parse_official_catalog_response(&raw_catalog, "ProxyCache") {
            return Ok(models);
        }
    }

    // If the cache wasn't populated (e.g. proxy bypassed or not intercepting),
    // fallback to parsing whatever the language server returned (which may be filtered).
    if request_succeeded {
        if ide_status.installed && ide_status.ide_running {
            if let Ok(models) = fetch_desktop_official_models(
                OfficialCatalogSource::Ide,
                RUNNING_HOST_RETRY_TIMEOUT,
            )
            .await
            {
                return Ok(models);
            }
        }
        if app_status.installed && app_status.app_running {
            if let Ok(models) = fetch_desktop_official_models(
                OfficialCatalogSource::App,
                RUNNING_HOST_RETRY_TIMEOUT,
            )
            .await
            {
                return Ok(models);
            }
        }
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
        modified_response: None,
    }
}

fn official_debug_proxy_failure(error: ProxyError) -> OfficialModelsDebugResult {
    OfficialModelsDebugResult {
        success: false,
        source: None,
        request_url: None,
        status_code: Some(error.status_code),
        content_type: None,
        error_category: Some(error.category.as_str().to_string()),
        error_message: Some(error.message),
        raw_response: error.upstream_body,
        modified_response: None,
    }
}

fn official_debug_success(
    raw: OfficialCatalogRawResponse,
    modified_response: String,
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
        modified_response: Some(modified_response),
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
    let proxy_target = proxy_runtime_snapshot(&state).await.endpoint;
    let (ide_status, app_status) = statuses;
    if !ide_status.installed && !app_status.installed {
        return Ok(official_debug_failure(
            "official_models_host_not_installed",
            "未检测到 Antigravity IDE 或 App",
            None,
        ));
    }

    let mut found_stopped_host = false;
    let mut last_error: Option<OfficialModelsDebugResult> = None;
    for (installed, running, source) in [
        (
            ide_status.installed,
            ide_status.ide_running,
            OfficialCatalogSource::Ide,
        ),
        (
            app_status.installed,
            app_status.app_running,
            OfficialCatalogSource::App,
        ),
    ] {
        if !installed {
            continue;
        }
        if !running {
            found_stopped_host = true;
            continue;
        }

        match fetch_desktop_official_models_raw(source, RUNNING_HOST_RETRY_TIMEOUT).await {
            Ok(raw) => {
                let actual_raw_body = raw.body.clone();
                match parse_official_catalog_response(&actual_raw_body, raw.source.as_str()) {
                    Ok(_) => {
                        let mut base_json: serde_json::Value =
                            match serde_json::from_str(&actual_raw_body) {
                                Ok(value) => value,
                                Err(error) => {
                                    last_error = Some(official_debug_failure(
                                        "upstream_server_error",
                                        format!("解析官方模型原始响应失败：{error}"),
                                        Some(actual_raw_body),
                                    ));
                                    continue;
                                }
                            };
                        if let Some(object) = base_json.as_object_mut() {
                            object.remove("error");
                        }
                        let proxy = ProxyServer::new(state.config_store.clone(), 0);
                        let modified_response =
                            proxy.prepare_model_catalog_response(base_json, &proxy_target);
                        let mut final_raw = raw;
                        final_raw.body = actual_raw_body;
                        return Ok(official_debug_success(final_raw, modified_response));
                    }
                    Err(error) => {
                        last_error = Some(official_debug_proxy_failure(error));
                    }
                }
            }
            Err(error) => {
                last_error = Some(official_debug_proxy_failure(error));
            }
        }
    }

    Ok(last_error.unwrap_or_else(|| {
        if found_stopped_host {
            official_debug_failure(
                "official_models_host_not_running",
                "请先启动 Antigravity IDE 或 App，再获取官方模型数据",
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
                request_body: None,
                response_body: None,
                error_message: Some(error),
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
    result: Result<agy_byok::domain::ConnectionTestContext, ProxyError>,
    duration_ms: u64,
) -> ModelConnectionTestResult {
    match result {
        Ok(context) => ModelConnectionTestResult {
            success: context.success,
            duration_ms,
            error_category: context.error_category.map(|c| c.as_str()),
            status_code: context.status_code,
            request_body: context.request_body,
            response_body: context.response_body,
            error_message: context.error_message,
        },
        Err(error) => ModelConnectionTestResult {
            success: false,
            duration_ms,
            error_category: Some(error.category.as_str()),
            status_code: Some(error.status_code),
            request_body: None,
            response_body: error.upstream_body,
            error_message: Some(error.message),
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
                } else if value.eq_ignore_ascii_case("adaptive") {
                    Ok(ReasoningMapping::Adaptive)
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
    let is_image_model = agy_byok::is_official_image_model_id(&upstream_model_id);
    let roles = if is_image_model {
        std::collections::BTreeSet::from([ModelRole::ImageGeneration])
    } else {
        std::collections::BTreeSet::from([ModelRole::Agent])
    };
    let output_modalities = if is_image_model {
        std::collections::BTreeSet::from([ModelModality::Image])
    } else {
        std::collections::BTreeSet::from([ModelModality::Text])
    };

    let config = AppConfig {
        proxy_port: DEFAULT_PROXY_PORT,
        providers: vec![provider],
        disabled_official_models: std::collections::HashSet::new(),
        upstream_models: vec![UpstreamModel {
            id: "preview-upstream".to_string(),
            provider_id,
            upstream_model_id,
            display_name: "连接预检模型".to_string(),
            capabilities: ModelCapabilities {
                roles,
                output_modalities,
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
    fn anthropic_preview_preserves_adaptive_custom_value() {
        assert_eq!(
            preview_reasoning_mapping(
                &ProviderProtocol::AnthropicMessages,
                ReasoningLevel::Auto,
                Some("adaptive"),
                None,
            )
            .unwrap(),
            ReasoningMapping::Adaptive
        );
    }
}
