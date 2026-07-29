use crate::domain::{
    ErrorCategory, NeutralChatRequest, ParameterOverrides, Provider, ProxyError, UpstreamModel,
    VirtualModel,
};
use crate::storage::AppConfig;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub virtual_model: VirtualModel,
    pub upstream_model: UpstreamModel,
    pub provider: Provider,
    pub final_parameters: ParameterOverrides,
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
            .find(|vm| vm.id == request.virtual_model_id && vm.enabled)
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

        // 如果包含思考变体，将思考变体设置包含到 extra_body 或合并逻辑中
        if let Some(ref variant) = virtual_model.reasoning_variant {
            let mut extra = final_parameters.extra_body.unwrap_or_default();
            extra.insert(
                variant.request_field.clone(),
                serde_json::Value::String(variant.request_value.clone()),
            );
            final_parameters.extra_body = Some(extra);
        }

        // 合并 Request 中的 extra_body
        if !request.extra_body.is_empty() {
            let mut extra = final_parameters.extra_body.unwrap_or_default();
            for (k, v) in &request.extra_body {
                extra.insert(k.clone(), v.clone());
            }
            Self::sanitize_extra_body(&mut extra);
            final_parameters.extra_body = Some(extra);
        }

        Ok(ResolvedRoute {
            virtual_model: virtual_model.clone(),
            upstream_model: upstream_model.clone(),
            provider: provider.clone(),
            final_parameters,
        })
    }

    /// 当遇到可重试失败且满足安全条件时，查找备用 VirtualModel 路由
    pub fn resolve_fallback(
        config: &AppConfig,
        failed_route: &ResolvedRoute,
    ) -> Result<Option<ResolvedRoute>, ProxyError> {
        let fallback_id = match &failed_route.virtual_model.fallback_virtual_model_id {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(None),
        };

        let dummy_request = NeutralChatRequest {
            virtual_model_id: fallback_id.clone(),
            messages: vec![],
            system_instruction: None,
            tools: vec![],
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        };

        let fallback_route = Self::resolve(config, &dummy_request)?;

        // 校验能力降级规则：备用模型的能力不得低于主模型
        let main_cap = &failed_route.upstream_model.capabilities;
        let fb_cap = &fallback_route.upstream_model.capabilities;

        if (main_cap.vision && !fb_cap.vision)
            || (main_cap.tools && !fb_cap.tools)
            || (main_cap.thinking && !fb_cap.thinking)
        {
            return Err(ProxyError::new(
                ErrorCategory::UnsupportedFeature,
                format!(
                    "Fallback model {} capabilities are lower than primary model {}",
                    fallback_route.virtual_model.id, failed_route.virtual_model.id
                ),
                400,
            ));
        }

        Ok(Some(fallback_route))
    }
}
