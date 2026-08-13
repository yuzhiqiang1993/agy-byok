use crate::domain::{
    AppConfig, ErrorCategory, NeutralChatRequest, ParameterOverrides, Provider, ProxyError,
    ReasoningLevel, UpstreamModel, VirtualModel,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub virtual_model: VirtualModel,
    pub upstream_model: UpstreamModel,
    pub provider: Provider,
    pub final_parameters: ParameterOverrides,
    pub final_reasoning_level: Option<ReasoningLevel>,
}

pub struct RouteTable;

impl RouteTable {
    /// 校验受控字段黑名单，防止用户设置的 extra_body 篡改关键系统字段
    pub fn sanitize_extra_body(extra_body: &mut HashMap<String, serde_json::Value>) {
        let blacklisted_keys = [
            "model",
            "messages",
            "contents",
            "stream",
            "tools",
            "functions",
            "authorization",
            "api-key",
            "x-api-key",
        ];
        for key in blacklisted_keys {
            extra_body.remove(key);
        }
    }

    /// 将 NeutralChatRequest 路由并解析为最终生效的 Provider、UpstreamModel 与合并后的 ParameterOverrides
    pub fn resolve(
        config: &AppConfig,
        request: &NeutralChatRequest,
    ) -> Result<ResolvedRoute, ProxyError> {
        let virtual_model = config
            .virtual_models
            .iter()
            .find(|vm| vm.enabled && vm.matches_id(&request.virtual_model_id))
            .or_else(|| resolve_tiered_fallback(config, &request.virtual_model_id))
            .or_else(|| resolve_image_generation_route(config, request))
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::ModelNotFound,
                    format!(
                        "Virtual model not found or disabled: {}",
                        request.virtual_model_id
                    ),
                    404,
                )
            })?;

        let upstream_model = config
            .upstream_models
            .iter()
            .find(|um| um.id == virtual_model.upstream_model_id && um.enabled)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::ModelNotFound,
                    format!(
                        "Upstream model not found or disabled: {}",
                        virtual_model.upstream_model_id
                    ),
                    404,
                )
            })?;

        let provider = config
            .providers
            .iter()
            .find(|p| p.id == upstream_model.provider_id && p.enabled)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::ModelNotFound,
                    format!(
                        "Provider not found or disabled: {}",
                        upstream_model.provider_id
                    ),
                    404,
                )
            })?;

        // 层层覆盖参数: Provider 默认 -> UpstreamModel -> VirtualModel -> Request 本次
        let mut final_parameters = provider
            .default_parameters
            .merge_with(&upstream_model.parameter_overrides)
            .merge_with(&virtual_model.parameter_overrides)
            .merge_with(&request.generation_parameters);

        let final_reasoning_level = request
            .reasoning_level
            .or(virtual_model.default_reasoning_level);
        if let Some(level) = final_reasoning_level {
            upstream_model
                .capabilities
                .reasoning
                .mapping_for(level)
                .ok_or_else(|| {
                    ProxyError::new(
                        ErrorCategory::UnsupportedFeature,
                        format!(
                            "Reasoning level {:?} is not supported by upstream model {}",
                            level, upstream_model.id
                        ),
                        400,
                    )
                })?;
        }

        // 合并 Request 中的 extra_body
        if !request.extra_body.is_empty() {
            let extra = final_parameters.extra_body.get_or_insert_default();
            for (k, v) in &request.extra_body {
                extra.insert(k.clone(), v.clone());
            }
        }
        if let Some(extra) = final_parameters.extra_body.as_mut() {
            Self::sanitize_extra_body(extra);
        }

        Ok(ResolvedRoute {
            virtual_model: virtual_model.clone(),
            upstream_model: upstream_model.clone(),
            provider: provider.clone(),
            final_parameters,
            final_reasoning_level,
        })
    }

    /// 当遇到可重试失败且满足安全条件时，查找备用 VirtualModel 路由
    pub fn resolve_fallback(
        config: &AppConfig,
        failed_route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<Option<ResolvedRoute>, ProxyError> {
        let fallback_id = match &failed_route.virtual_model.fallback_virtual_model_id {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(None),
        };

        let mut fallback_request = request.clone();
        fallback_request.virtual_model_id = fallback_id.clone();
        fallback_request.reasoning_level = failed_route.final_reasoning_level;

        let fallback_route = Self::resolve(config, &fallback_request)?;

        // 能力是用户声明而非本地事实；备用模型是否接受当前请求交由上游判断。
        Ok(Some(fallback_route))
    }
}

/// 当请求 ID 是母条目（如 `*-tiered`）时，自动查找并 Fallback 到该模型已启用的默认档位（优先 High -> Medium -> Low）。
fn resolve_tiered_fallback<'a>(
    config: &'a AppConfig,
    requested_id: &str,
) -> Option<&'a VirtualModel> {
    let base_id = requested_id.strip_suffix("-tiered")?;
    let candidates: Vec<&'a VirtualModel> = config
        .virtual_models
        .iter()
        .filter(|vm| vm.enabled)
        .filter(|vm| {
            let cat_key = vm.catalog_key();
            let cat_base = strip_known_level_suffix(cat_key.as_ref());
            let id_base = strip_known_level_suffix(vm.id.as_str());
            cat_base == base_id || id_base == base_id || cat_key.starts_with(base_id) || vm.id.starts_with(base_id)
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // 默认档位优先级: High -> Medium -> Low -> XHigh -> Max -> 其它
    const PREFERRED_ORDER: &[ReasoningLevel] = &[
        ReasoningLevel::High,
        ReasoningLevel::Medium,
        ReasoningLevel::Low,
        ReasoningLevel::XHigh,
        ReasoningLevel::Max,
        ReasoningLevel::Adaptive,
        ReasoningLevel::Auto,
        ReasoningLevel::Off,
    ];

    for preferred in PREFERRED_ORDER {
        if let Some(vm) = candidates
            .iter()
            .find(|vm| vm.default_reasoning_level == Some(*preferred))
        {
            return Some(*vm);
        }
    }

    candidates.first().copied()
}

fn strip_known_level_suffix(id: &str) -> &str {
    const LEVELS: &[&str] = &[
        "-adaptive", "-x-high", "-medium", "-auto", "-high", "-max", "-low", "-off",
    ];
    for suffix in LEVELS {
        if let Some(base) = id.strip_suffix(suffix) {
            return base;
        }
    }
    id
}

/// 判断模型 ID 是否属于官方/系统内置生图模型标识
pub(crate) fn is_official_image_model_id(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    lower.contains("flash-image")
        || lower.contains("imagen")
        || lower.contains("nano-banana-pro")
        || lower.contains("image-generation")
        || lower.contains("dall-e")
        || lower.contains("flux")
        || lower.contains("midjourney")
        || lower.contains("sdxl")
        || lower.contains("stable-diffusion")
        || lower.contains("recraft")
        || lower.contains("kolors")
}

/// 查找当前配置中已启用的自定义生图模型
pub(crate) fn find_active_custom_image_model(config: &AppConfig) -> Option<&VirtualModel> {
    config.virtual_models.iter().find(|vm| {
        if !vm.enabled {
            return false;
        }
        config.upstream_models.iter().any(|um| {
            um.id == vm.upstream_model_id
                && um.enabled
                && um
                    .capabilities
                    .roles
                    .contains(&crate::domain::ModelRole::ImageGeneration)
        })
    })
}

/// 当请求带有图片输出诉求或直接请求官方生图模型时，重定向到用户启用的自定义生图模型
fn resolve_image_generation_route<'a>(
    config: &'a AppConfig,
    request: &NeutralChatRequest,
) -> Option<&'a VirtualModel> {
    let wants_image = request
        .output_modalities
        .contains(&crate::domain::ModelModality::Image)
        || is_official_image_model_id(&request.virtual_model_id);
    if wants_image {
        find_active_custom_image_model(config)
    } else {
        None
    }
}

