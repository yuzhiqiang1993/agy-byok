pub(crate) mod anthropic;
pub(crate) mod catalog;
mod error;
pub(crate) mod gemini;
pub(crate) mod openai;
pub(crate) mod openai_responses;
pub(crate) mod traits;

pub(crate) use anthropic::AnthropicAdapter;
pub use catalog::{
    fetch_official_models_catalog, fetch_provider_models, OfficialCatalogSource,
    ProviderCatalogModel,
};
pub(crate) use gemini::GeminiAdapter;
pub(crate) use openai::OpenAIAdapter;
pub(crate) use openai_responses::OpenAIResponsesAdapter;
pub(crate) use traits::{ProviderAdapter, ProviderStreamDecoder};

use crate::domain::ProviderProtocol;
use std::sync::Arc;

pub(crate) fn get_adapter(protocol: &ProviderProtocol) -> Arc<dyn ProviderAdapter> {
    match protocol {
        ProviderProtocol::OpenaiChatCompletions => Arc::new(OpenAIAdapter::new()),
        ProviderProtocol::AnthropicMessages => Arc::new(AnthropicAdapter::new()),
        ProviderProtocol::GeminiGenerateContent => Arc::new(GeminiAdapter::new()),
        ProviderProtocol::OpenaiResponses => Arc::new(OpenAIResponsesAdapter::new()),
    }
}
