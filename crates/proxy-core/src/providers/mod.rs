pub mod anthropic;
pub mod catalog;
mod error;
pub mod gemini;
pub mod openai;
pub mod openai_responses;
pub mod traits;

pub use anthropic::AnthropicAdapter;
pub use catalog::{fetch_provider_models, ProviderCatalogModel};
pub use gemini::GeminiAdapter;
pub use openai::OpenAIAdapter;
pub use openai_responses::OpenAIResponsesAdapter;
pub use traits::{ProviderAdapter, ProviderStreamDecoder};

use crate::domain::ProviderProtocol;
use std::sync::Arc;

pub fn get_adapter(protocol: &ProviderProtocol) -> Arc<dyn ProviderAdapter> {
    match protocol {
        ProviderProtocol::OpenaiChatCompletions => Arc::new(OpenAIAdapter::new()),
        ProviderProtocol::AnthropicMessages => Arc::new(AnthropicAdapter::new()),
        ProviderProtocol::GeminiGenerateContent => Arc::new(GeminiAdapter::new()),
        ProviderProtocol::OpenaiResponses => Arc::new(OpenAIResponsesAdapter::new()),
    }
}
