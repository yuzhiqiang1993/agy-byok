use crate::domain::{
    strip_reasoning_level_suffix, AppConfig, ErrorCategory, NeutralChatRequest, ParameterOverrides,
    Provider, ProxyError, ReasoningLevel, UpstreamModel, VirtualModel, REASONING_LEVEL_PRIORITY,
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
            .find(|vm| {
                is_routable_virtual_model(config, vm) && vm.matches_id(&request.virtual_model_id)
            })
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

/// 当请求 ID 是母条目（如 `*-tiered`）或 Base Slug 时，查找该模型族已启用的默认档位（优先 High -> Medium -> Low）。未知自定义 ID 保持本地失败，不猜测模型族。
fn resolve_tiered_fallback<'a>(
    config: &'a AppConfig,
    requested_id: &str,
) -> Option<&'a VirtualModel> {
    let clean_id = requested_id.strip_prefix("models/").unwrap_or(requested_id);
    let base_id = clean_id
        .strip_suffix("-tiered")
        .unwrap_or_else(|| strip_reasoning_level_suffix(clean_id));

    let candidates: Vec<&'a VirtualModel> = config
        .virtual_models
        .iter()
        .filter(|vm| is_routable_virtual_model(config, vm))
        .filter(|vm| {
            model_family_base(vm.id.as_str()) == base_id
                || model_family_base(vm.catalog_key().as_ref()) == base_id
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // 默认档位优先级: High -> Medium -> Low -> XHigh -> Max -> 其它
    for preferred in REASONING_LEVEL_PRIORITY {
        if let Some(vm) = candidates
            .iter()
            .find(|vm| vm.default_reasoning_level == Some(*preferred))
        {
            return Some(*vm);
        }
    }

    candidates.first().copied()
}

fn model_family_base(id: &str) -> &str {
    strip_reasoning_level_suffix(id.strip_suffix("-tiered").unwrap_or(id))
}

fn is_routable_virtual_model(config: &AppConfig, virtual_model: &VirtualModel) -> bool {
    if !virtual_model.enabled {
        return false;
    }
    let Some(upstream_model) = config
        .upstream_models
        .iter()
        .find(|model| model.id == virtual_model.upstream_model_id)
    else {
        return false;
    };
    upstream_model.enabled
        && config
            .providers
            .iter()
            .any(|provider| provider.id == upstream_model.provider_id && provider.enabled)
}

pub fn matches_custom_model_id(config: &AppConfig, model_id: &str) -> bool {
    let clean_id = model_id.strip_prefix("models/").unwrap_or(model_id);
    if clean_id.starts_with("custom-") {
        return true;
    }
    if config
        .virtual_models
        .iter()
        .any(|virtual_model| virtual_model.matches_id(clean_id))
    {
        return true;
    }
    let base_id = model_family_base(clean_id);
    config.virtual_models.iter().any(|virtual_model| {
        model_family_base(virtual_model.id.as_str()) == base_id
            || model_family_base(virtual_model.catalog_key().as_ref()) == base_id
    })
}

/// 判断模型 ID 是否属于官方/系统内置生图模型标识
pub fn is_official_image_model_id(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    lower.contains("flash-image")
        || lower.contains("imagen")
        || lower.contains("nano-banana-pro")
        || lower.contains("image-generation")
        || lower.contains("image_generation")
        || lower.contains("text-to-image")
        || lower.contains("text2image")
        || lower.contains("image-to-image")
        || lower.contains("image2image")
        || lower.contains("text-to-video")
        || lower.contains("text2video")
        || lower.contains("dall-e")
        || lower.contains("dalle")
        || lower.contains("gpt-image")
        || lower.contains("gpt_image")
        || lower.contains("flux")
        || lower.contains("midjourney")
        || lower.contains("sdxl")
        || lower.contains("stable-diffusion")
        || lower.contains("stable_diffusion")
        || lower.contains("stable-image")
        || lower.contains("recraft")
        || lower.contains("kolors")
        || lower.contains("ideogram")
        || lower.contains("kling")
        || lower.contains("cogview")
        || lower.contains("imagine")
        || lower.contains("hunyuan-image")
        || lower.contains("hunyuan-video")
        || lower.contains("doubao-image")
        || lower.contains("wanx")
        || is_image_version_pattern(&lower)
}

fn is_image_version_pattern(lower: &str) -> bool {
    if let Some(idx) = lower.find("image") {
        let rest = &lower[idx + 5..];
        let rest = rest.trim_start_matches(['-', '_', ' ']);
        if let Some(first_char) = rest.chars().next() {
            return first_char.is_ascii_digit() || first_char == 'v';
        }
    }
    false
}

/// 查找当前配置中已启用的自定义生图模型
pub(crate) fn find_active_custom_image_model(config: &AppConfig) -> Option<&VirtualModel> {
    config.virtual_models.iter().find(|vm| {
        if !vm.enabled {
            return false;
        }
        config
            .upstream_models
            .iter()
            .find(|um| um.id == vm.upstream_model_id && um.enabled)
            .is_some_and(|um| {
                um.capabilities
                    .roles
                    .contains(&crate::domain::ModelRole::ImageGeneration)
                    && config
                        .providers
                        .iter()
                        .any(|provider| provider.id == um.provider_id && provider.enabled)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_official_image_model_id() {
        assert!(is_official_image_model_id("gpt-image-1.5"));
        assert!(is_official_image_model_id("gpt-image-2"));
        assert!(is_official_image_model_id("grok-imagine-video-1.5"));
        assert!(is_official_image_model_id("dall-e-3"));
        assert!(is_official_image_model_id("dalle-3"));
        assert!(is_official_image_model_id("flux-schnell"));
        assert!(is_official_image_model_id("midjourney-v6"));
        assert!(is_official_image_model_id("imagen-3"));
        assert!(is_official_image_model_id("stable-diffusion-3"));
        assert!(is_official_image_model_id("doubao-image"));
        assert!(is_official_image_model_id("kling-v1"));

        // 普通对话模型不应被误判为生图模型
        assert!(!is_official_image_model_id("gpt-4o"));
        assert!(!is_official_image_model_id("claude-3-5-sonnet"));
        assert!(!is_official_image_model_id("deepseek-chat"));
        assert!(!is_official_image_model_id("grok-4.5"));
    }
}
