use super::ProxyServer;
use crate::antigravity::AntigravityModelDescriptor;
use crate::domain::ReasoningLevel;
use crate::proxy::http_server::forwarding::rewrite_official_urls_str;

fn strip_ascii_case_insensitive_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let start = value.len().checked_sub(suffix.len())?;
    let suffix_value = value.get(start..)?;
    if !suffix_value.eq_ignore_ascii_case(suffix) {
        return None;
    }
    value.get(..start)
}

impl ProxyServer {
    pub(crate) fn is_custom_model_id(&self, model_id: &str) -> bool {
        let clean_id = model_id.strip_prefix("models/").unwrap_or(model_id);
        let config = self.config_store.get_config();
        clean_id.starts_with("custom-")
            || config
                .virtual_models
                .iter()
                .any(|model| model.matches_id(clean_id) || model.matches_id(model_id))
            || (crate::routing::route_table::is_official_image_model_id(clean_id)
                && crate::routing::route_table::find_active_custom_image_model(&config).is_some())
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
            &config.providers,
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
    let mut base_name = model_name.trim();
    base_name = strip_ascii_case_insensitive_suffix(base_name, &provider_suffix)
        .unwrap_or(base_name)
        .trim_end();
    // 兼容旧版目录中的 `Model high(Provider)`，以及官方风格的
    // `Model (High)`，避免重复保存配置后不断叠加档位后缀。
    for known_reasoning in [
        "default", "off", "low", "medium", "high", "xhigh", "x-high", "max", "adaptive", "auto",
        "custom",
    ] {
        for suffix in [
            format!(" ({known_reasoning})"),
            format!(" {known_reasoning}"),
        ] {
            if let Some(stripped) = strip_ascii_case_insensitive_suffix(base_name, &suffix) {
                base_name = stripped.trim_end();
                break;
            }
        }
    }
    if !supports_reasoning {
        return format!("{base_name}{provider_suffix}");
    }

    let Some(reasoning) = reasoning_level else {
        // 供应商通过 tagTitle/tagDescription 展示；推理模型的主条目不再
        // 把 `default(Provider)` 混入名称，否则官方 IDE 无法聚类档位。
        return base_name.to_string();
    };
    let reasoning = match reasoning {
        ReasoningLevel::Off => "Off",
        ReasoningLevel::Low => "Low",
        ReasoningLevel::Medium => "Medium",
        ReasoningLevel::High => "High",
        ReasoningLevel::XHigh => "X-High",
        ReasoningLevel::Max => "Max",
        ReasoningLevel::Adaptive => "Adaptive",
        ReasoningLevel::Auto => "Custom",
    };
    // Antigravity 当前模型选择器按 `Base (Low|Medium|High)` 聚类，
    // 供应商信息由模型目录的 tagTitle/tagDescription 单独承载。
    format!("{base_name} ({reasoning})")
}
