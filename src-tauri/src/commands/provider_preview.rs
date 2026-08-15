use agy_byok::domain::{
    AppConfig, ModelCapabilities, ModelModality, ModelRole, ModelTokenLimits, ParameterOverrides,
    Provider, ProviderProtocol, ProxyError, ReasoningCapability, ReasoningLevel, ReasoningMapping,
    UpstreamModel, VirtualModel, DEFAULT_PROXY_PORT,
};
use std::collections::{BTreeSet, HashSet};

use super::provider::ModelConnectionTestResult;

pub(super) fn connection_test_result(
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

pub(super) fn preview_reasoning_mapping(
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

pub(super) fn preview_model_config(
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
        BTreeSet::from([ModelRole::ImageGeneration])
    } else {
        BTreeSet::from([ModelRole::Agent])
    };
    let output_modalities = if is_image_model {
        BTreeSet::from([ModelModality::Image])
    } else {
        BTreeSet::from([ModelModality::Text])
    };

    let config = AppConfig {
        proxy_port: DEFAULT_PROXY_PORT,
        providers: vec![provider],
        disabled_official_models: HashSet::new(),
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
