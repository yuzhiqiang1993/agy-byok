pub(crate) mod anthropic;
pub(crate) mod catalog;
mod error;
pub(crate) mod gemini;
pub(crate) mod openai;
pub(crate) mod openai_responses;
pub(crate) mod traits;

pub(crate) use anthropic::AnthropicAdapter;
pub use catalog::{
    fetch_official_models_catalog, fetch_official_models_catalog_raw, fetch_provider_models,
    fetch_provider_models_raw, parse_official_catalog_response, OfficialCatalogRawResponse,
    OfficialCatalogSource, ProviderCatalogModel, ProviderCatalogRawResponse,
};
pub(crate) use gemini::GeminiAdapter;
pub(crate) use openai::OpenAIAdapter;
pub(crate) use openai_responses::OpenAIResponsesAdapter;
pub(crate) use traits::{ProviderAdapter, ProviderStreamDecoder};

use crate::domain::{
    ModelModality, ModelRole, NeutralChatRequest, ProviderProtocol, UpstreamModel,
};
use std::sync::Arc;

pub(crate) fn get_adapter(protocol: &ProviderProtocol) -> Arc<dyn ProviderAdapter> {
    match protocol {
        ProviderProtocol::OpenaiChatCompletions => Arc::new(OpenAIAdapter::new()),
        ProviderProtocol::AnthropicMessages => Arc::new(AnthropicAdapter::new()),
        ProviderProtocol::GeminiGenerateContent => Arc::new(GeminiAdapter::new()),
        ProviderProtocol::OpenaiResponses => Arc::new(OpenAIResponsesAdapter::new()),
    }
}

/// 判断本次请求是否为「图片生成」请求。
///
/// 图片生成在 Antigravity 里通过 Gemini 传输链路下发（`responseModalities=IMAGE`
/// 与 `imageConfig`）。Gemini 上游原生支持该能力；OpenAI 需要切换到独立的
/// images 端点；Anthropic 官方不支持，应由适配器返回明确错误而不是静默丢弃。
pub(crate) fn is_image_generation_request(
    upstream_model: &UpstreamModel,
    request: &NeutralChatRequest,
) -> bool {
    let roles = &upstream_model.capabilities.roles;
    if !roles.contains(&ModelRole::ImageGeneration) {
        return false;
    }

    let wants_image = request.output_modalities.contains(&ModelModality::Image);
    let wants_text = request.output_modalities.contains(&ModelModality::Text);
    let is_agent = roles.contains(&ModelRole::Agent);

    // 请求明确要求图片输出（即使是 text+image 混合，图片部分只能由 images 端点满足）。
    if wants_image {
        return true;
    }
    // 纯生图模型（未声明 agent 对话角色），且请求未明确只要文本。
    if !is_agent && !wants_text {
        return true;
    }
    false
}
