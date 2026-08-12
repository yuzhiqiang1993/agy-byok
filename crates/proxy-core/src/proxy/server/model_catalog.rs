use super::ProxyServer;
use crate::antigravity::AntigravityModelDescriptor;
use crate::domain::ReasoningLevel;
use crate::proxy::http_server::forwarding::rewrite_official_urls_str;

impl ProxyServer {
    pub(crate) fn is_custom_model_id(&self, model_id: &str) -> bool {
        self.config_store
            .get_config()
            .virtual_models
            .iter()
            .any(|model| model.matches_id(model_id))
            || model_id.starts_with("custom-")
    }

    /// 使用与模型目录代理响应相同的完整链路生成最终 JSON。
    pub fn prepare_model_catalog_response(
        &self,
        base_json: serde_json::Value,
        proxy_target: &str,
    ) -> String {
        let models = self.handle_model_list(base_json);
        rewrite_official_urls_str(&models.to_string(), proxy_target)
    }

    /// 注入并融合包含自定义虚拟模型的模型列表描述 JSON
    pub(crate) fn handle_model_list(&self, mut base_json: serde_json::Value) -> serde_json::Value {
        let config = self.config_store.get_config();
        let catalog_virtual_models = config
            .virtual_models
            .iter()
            .filter(|virtual_model| {
                if !virtual_model.enabled {
                    return false;
                }
                let Some(upstream_model) = config
                    .upstream_models
                    .iter()
                    .find(|upstream| upstream.id == virtual_model.upstream_model_id)
                else {
                    return false;
                };
                upstream_model.enabled
                    && config.providers.iter().any(|provider| {
                        provider.id == upstream_model.provider_id && provider.enabled
                    })
            })
            .cloned()
            .map(|mut virtual_model| {
                let upstream_model = config
                    .upstream_models
                    .iter()
                    .find(|upstream| upstream.id == virtual_model.upstream_model_id);
                if let Some(upstream_model) = upstream_model {
                    let provider = config
                        .providers
                        .iter()
                        .find(|provider| provider.id == upstream_model.provider_id);
                    if let Some(provider) = provider {
                        virtual_model.display_name = configured_model_display_name(
                            &virtual_model.display_name,
                            virtual_model.default_reasoning_level,
                            &provider.name,
                            upstream_model.capabilities.reasoning.supports_reasoning(),
                        );
                    }
                }
                virtual_model
            })
            .collect::<Vec<_>>();
        AntigravityModelDescriptor::remove_disabled_official_models(
            &mut base_json,
            &config.disabled_official_models,
        );
        AntigravityModelDescriptor::apply_official_model_overrides(
            &mut base_json,
            &config.model_compression_policies,
        );
        AntigravityModelDescriptor::inject_into_model_list(
            &mut base_json,
            &catalog_virtual_models,
            &config.upstream_models,
        );
        base_json
    }
}

pub(super) fn configured_model_display_name(
    model_name: &str,
    reasoning_level: Option<ReasoningLevel>,
    provider_name: &str,
    supports_reasoning: bool,
) -> String {
    let provider_suffix = format!("({provider_name})");
    let mut base_name = model_name;
    for known_reasoning in [
        "default", "off", "low", "medium", "high", "xhigh", "max", "adaptive", "auto",
    ] {
        let known_suffix = format!(" {known_reasoning}({provider_name})");
        if let Some(stripped) = base_name.strip_suffix(&known_suffix) {
            base_name = stripped;
            break;
        }
    }
    base_name = base_name
        .strip_suffix(&provider_suffix)
        .unwrap_or(base_name);
    if !supports_reasoning {
        return format!("{base_name}{provider_suffix}");
    }

    let reasoning = match reasoning_level {
        None => "default",
        Some(ReasoningLevel::Off) => "off",
        Some(ReasoningLevel::Low) => "low",
        Some(ReasoningLevel::Medium) => "medium",
        Some(ReasoningLevel::High) => "high",
        Some(ReasoningLevel::XHigh) => "xhigh",
        Some(ReasoningLevel::Max) => "max",
        Some(ReasoningLevel::Adaptive) => "adaptive",
        Some(ReasoningLevel::Auto) => "custom",
    };
    format!("{base_name} {reasoning}({provider_name})")
}
