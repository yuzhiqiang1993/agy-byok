use super::provider::ParameterOverrides;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub tools: bool,
    pub thinking: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            vision: true,
            tools: true,
            thinking: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamModel {
    pub id: String,
    pub provider_id: String,
    pub upstream_model_id: String,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
    pub parameter_overrides: ParameterOverrides,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningVariant {
    pub label: String,         // "Medium" | "High" | "XHigh" | "Max"
    pub request_field: String, // 如 "reasoning_effort"
    pub request_value: String, // 如 "high"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualModel {
    pub id: String, // 持久化 UUID
    pub upstream_model_id: String,
    pub display_name: String,
    pub reasoning_variant: Option<ReasoningVariant>,
    pub parameter_overrides: ParameterOverrides,
    pub fallback_virtual_model_id: Option<String>,
    pub enabled: bool,
}
