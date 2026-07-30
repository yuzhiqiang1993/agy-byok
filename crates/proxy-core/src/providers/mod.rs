pub mod anthropic;
pub mod catalog;
pub mod gemini;
pub mod openai;
pub mod traits;

pub use anthropic::AnthropicAdapter;
pub use catalog::{fetch_provider_models, ProviderCatalogModel};
pub use gemini::GeminiAdapter;
pub use openai::OpenAIAdapter;
pub use traits::{ProviderAdapter, ProviderStreamDecoder};

use crate::domain::ProviderProtocol;
use std::sync::Arc;

pub fn get_adapter(protocol: &ProviderProtocol) -> Arc<dyn ProviderAdapter> {
    match protocol {
        ProviderProtocol::Openai => Arc::new(OpenAIAdapter::new()),
        ProviderProtocol::Anthropic => Arc::new(AnthropicAdapter::new()),
        ProviderProtocol::Gemini => Arc::new(GeminiAdapter::new()),
    }
}
