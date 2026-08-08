use crate::commands::error::PROVIDER_CATALOG_FAILED;
use crate::state::DesktopState;
use agy_byok::domain::{
    AppConfig, ModelCapabilities, ModelTokenLimits, ParameterOverrides, Provider, ProviderProtocol,
    ProxyError, ReasoningCapability, ReasoningLevel, ReasoningMapping, UpstreamModel, VirtualModel,
    DEFAULT_PROXY_PORT,
};
use agy_byok::providers::{fetch_provider_models, ProviderCatalogModel};
use agy_byok::proxy::ProxyServer;
use agy_byok::storage::ConfigStore;
use serde::Serialize;
use std::time::Instant;
use tauri::State;

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
            checkpoint_override: None,
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
        official_model_settings: Default::default(),
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
}
