use crate::domain::{
    AppConfig, ErrorCategory, NeutralChatRequest, ParameterOverrides, Provider, ProxyError,
    ReasoningLevel, UpstreamModel, VirtualModel,
};
use std::collections::HashMap;

use super::image_route::resolve_image_generation_route;
use super::tiered_fallback::{is_routable_virtual_model, resolve_tiered_fallback};

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
