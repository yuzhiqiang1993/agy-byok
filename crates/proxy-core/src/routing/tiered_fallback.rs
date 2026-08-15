use crate::domain::{
    model_family_base, strip_reasoning_level_suffix, AppConfig, VirtualModel, CUSTOM_MODEL_PREFIX,
    MODEL_NAMESPACE_PREFIX, REASONING_LEVEL_PRIORITY,
};

/// 校验模型是否满足启用且其关联的上游及供应商均可用
pub fn is_routable_virtual_model(config: &AppConfig, virtual_model: &VirtualModel) -> bool {
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

/// 当请求 ID 是母条目（如 `*-tiered`）或 Base Slug 时，查找该模型族已启用的默认档位（优先 High -> Medium -> Low）。未知自定义 ID 保持本地失败，不猜测模型族。
pub(crate) fn resolve_tiered_fallback<'a>(
    config: &'a AppConfig,
    requested_id: &str,
) -> Option<&'a VirtualModel> {
    let clean_id = requested_id
        .strip_prefix(MODEL_NAMESPACE_PREFIX)
        .unwrap_or(requested_id);
    if config
        .virtual_models
        .iter()
        .any(|virtual_model| virtual_model.matches_id(clean_id))
    {
        // An exact configured identifier must not be reinterpreted as a
        // family request when that VirtualModel is disabled or unavailable.
        return None;
    }
    let is_tiered_parent = clean_id.ends_with("-tiered");
    let base_id = if let Some(base_id) = clean_id.strip_suffix("-tiered") {
        base_id
    } else {
        let base_id = strip_reasoning_level_suffix(clean_id);
        // A concrete reasoning variant must resolve exactly. Only a tiered
        // parent or an unsuffixed base slug may choose a preferred variant.
        if base_id != clean_id {
            return None;
        }
        base_id
    };

    // A slot-backed catalog key can represent only one concrete variant (for
    // example an `x_high` ID). Older tiered parents were generated from that
    // key, so recover their complete tier through the matching model family.
    let tiered_family_bases = if is_tiered_parent {
        config
            .virtual_models
            .iter()
            .filter(|vm| vm.matches_family_base(base_id))
            .map(|vm| vm.catalog_family_base().into_owned())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let candidates: Vec<&'a VirtualModel> = config
        .virtual_models
        .iter()
        .filter(|vm| is_routable_virtual_model(config, vm))
        .filter(|vm| {
            vm.matches_family_base(base_id)
                || (is_tiered_parent
                    && tiered_family_bases
                        .iter()
                        .any(|family_base| vm.catalog_family_base().as_ref() == family_base))
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

pub fn matches_custom_model_id(config: &AppConfig, model_id: &str) -> bool {
    let clean_id = model_id
        .strip_prefix(MODEL_NAMESPACE_PREFIX)
        .unwrap_or(model_id);
    if clean_id.starts_with(CUSTOM_MODEL_PREFIX) {
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
    config
        .virtual_models
        .iter()
        .any(|virtual_model| virtual_model.matches_family_base(base_id))
}
