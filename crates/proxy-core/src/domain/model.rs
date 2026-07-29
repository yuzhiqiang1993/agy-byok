use super::provider::ParameterOverrides;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningLevel {
    Off,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReasoningMapping {
    Disabled,
    Effort(String),
    BudgetTokens(u32),
    Adaptive,
    NativeLevel(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReasoningCapability {
    pub levels: BTreeMap<ReasoningLevel, ReasoningMapping>,
}

impl ReasoningCapability {
    pub fn mapping_for(&self, level: ReasoningLevel) -> Option<&ReasoningMapping> {
        self.levels.get(&level)
    }

    pub fn supports_reasoning(&self) -> bool {
        !self.levels.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: ReasoningCapability,
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
pub struct VirtualModel {
    pub id: String,
    pub upstream_model_id: String,
    pub display_name: String,
    pub default_reasoning_level: Option<ReasoningLevel>,
    pub parameter_overrides: ParameterOverrides,
    pub fallback_virtual_model_id: Option<String>,
    pub enabled: bool,
}
