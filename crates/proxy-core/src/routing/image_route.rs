use crate::domain::{AppConfig, ModelModality, ModelRole, NeutralChatRequest, VirtualModel};

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
pub fn find_active_custom_image_model(config: &AppConfig) -> Option<&VirtualModel> {
    config.virtual_models.iter().find(|vm| {
        if !vm.enabled {
            return false;
        }
        config
            .upstream_models
            .iter()
            .find(|um| um.id == vm.upstream_model_id && um.enabled)
            .is_some_and(|um| {
                um.capabilities.roles.contains(&ModelRole::ImageGeneration)
                    && config
                        .providers
                        .iter()
                        .any(|provider| provider.id == um.provider_id && provider.enabled)
            })
    })
}

/// 当请求带有图片输出诉求或直接请求官方生图模型时，重定向到用户启用的自定义生图模型
pub(crate) fn resolve_image_generation_route<'a>(
    config: &'a AppConfig,
    request: &NeutralChatRequest,
) -> Option<&'a VirtualModel> {
    let wants_image = request.output_modalities.contains(&ModelModality::Image)
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
