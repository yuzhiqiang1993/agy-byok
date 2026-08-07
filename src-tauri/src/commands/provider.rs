use crate::state::DesktopState;
use agy_byok::domain::{
    ErrorCategory, ModelCapabilities, ModelTokenLimits, ParameterOverrides, Provider,
    ProviderProtocol, ProxyError, ReasoningCapability, ReasoningLevel, ReasoningMapping,
    UpstreamModel, VirtualModel,
};
use agy_byok::providers::{fetch_provider_models, ProviderCatalogModel};
use agy_byok::proxy::ProxyServer;
use agy_byok::storage::{AppConfig, ConfigStore, DEFAULT_PROXY_PORT};
use serde::Serialize;
use std::time::Instant;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionTestResult {
    pub success: bool,
    pub duration_ms: u64,
    pub message: String,
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
pub(crate) async fn fetch_provider_catalog(
    provider: Provider,
) -> Result<Vec<ProviderCatalogModel>, String> {
    fetch_provider_models(&provider)
        .await
        .map_err(|error| model_connection_error_message(&error))
}

#[tauri::command]
pub(crate) async fn test_provider_model_connection(
    provider: Provider,
    upstream_model_id: String,
    reasoning_level: Option<ReasoningLevel>,
    custom_reasoning_value: Option<String>,
    reasoning_mapping: Option<ReasoningMapping>,
) -> Result<ModelConnectionTestResult, String> {
    let started = Instant::now();
    let config = preview_model_config(
        provider,
        upstream_model_id,
        reasoning_level,
        custom_reasoning_value.as_deref(),
        reasoning_mapping.as_ref(),
    )?;
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

    let _ = protocol;
    let _ = level;
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
        ErrorCategory::ContextLengthExceeded => {
            format!("请求上下文超过模型限制（HTTP {}）", error.status_code)
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
