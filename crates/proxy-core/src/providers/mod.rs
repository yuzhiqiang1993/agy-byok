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
    let is_agent = roles.contains(&ModelRole::Agent);

    if wants_image {
        return true;
    }
    if !is_agent {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        MessageRole, ModelCapabilities, NeutralContentBlock, NeutralMessage, ParameterOverrides,
    };
    use std::collections::BTreeSet;

    fn make_test_upstream_model(roles: BTreeSet<ModelRole>) -> UpstreamModel {
        UpstreamModel {
            id: "test-model".to_string(),
            provider_id: "test-provider".to_string(),
            upstream_model_id: "test-upstream".to_string(),
            display_name: "Test Model".to_string(),
            capabilities: ModelCapabilities {
                roles,
                ..Default::default()
            },
            token_limits: Default::default(),
            compression_policy: None,
            tokenizer: None,
            parameter_overrides: Default::default(),
            enabled: true,
        }
    }

    fn make_test_request(modalities: BTreeSet<ModelModality>) -> NeutralChatRequest {
        NeutralChatRequest {
            virtual_model_id: "test-virtual".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks: vec![NeutralContentBlock::Text("hi".to_string())],
            }],
            system_instruction: None,
            tools: vec![],
            output_modalities: modalities,
            image_generation_config: None,
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: Default::default(),
        }
    }

    #[test]
    fn test_image_generation_classification() {
        // 1. 无生图角色模型：无论请求模态为何，均不是生图请求
        let agent_only = make_test_upstream_model(BTreeSet::from([ModelRole::Agent]));
        assert!(!is_image_generation_request(
            &agent_only,
            &make_test_request(BTreeSet::new())
        ));
        assert!(!is_image_generation_request(
            &agent_only,
            &make_test_request(BTreeSet::from([ModelModality::Image]))
        ));

        // 2. 纯生图角色模型：无论请求是否显式声明 Image 模态，都是生图请求
        let image_only = make_test_upstream_model(BTreeSet::from([ModelRole::ImageGeneration]));
        assert!(is_image_generation_request(
            &image_only,
            &make_test_request(BTreeSet::new())
        ));
        assert!(is_image_generation_request(
            &image_only,
            &make_test_request(BTreeSet::from([ModelModality::Image]))
        ));

        // 3. 双角色模型（Agent + ImageGeneration）：仅当请求显式要求 Image 模态时才作为生图请求
        let dual_role = make_test_upstream_model(BTreeSet::from([
            ModelRole::Agent,
            ModelRole::ImageGeneration,
        ]));
        assert!(!is_image_generation_request(
            &dual_role,
            &make_test_request(BTreeSet::new())
        ));
        assert!(is_image_generation_request(
            &dual_role,
            &make_test_request(BTreeSet::from([ModelModality::Image]))
        ));
    }
}
