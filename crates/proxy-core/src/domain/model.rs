use super::provider::ParameterOverrides;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
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
    #[serde(default)]
    pub host_model_id: Option<String>,
    pub upstream_model_id: String,
    pub display_name: String,
    pub default_reasoning_level: Option<ReasoningLevel>,
    pub parameter_overrides: ParameterOverrides,
    pub fallback_virtual_model_id: Option<String>,
    pub enabled: bool,
}

impl VirtualModel {
    pub fn effective_host_model_id(&self) -> Cow<'_, str> {
        match &self.host_model_id {
            Some(host_model_id) => Cow::Borrowed(host_model_id),
            None => Cow::Owned(format!(
                "MODEL_PLACEHOLDER_M{}",
                400 + stable_hash(&self.id) % 200
            )),
        }
    }

    pub fn catalog_key(&self) -> Cow<'_, str> {
        if self.id.starts_with("custom-") {
            Cow::Borrowed(self.id.as_str())
        } else {
            Cow::Owned(format!("custom-{}", self.id))
        }
    }

    pub fn accepted_ids(&self) -> [Cow<'_, str>; 3] {
        [
            Cow::Borrowed(self.id.as_str()),
            self.effective_host_model_id(),
            self.catalog_key(),
        ]
    }

    pub fn matches_id(&self, model_id: &str) -> bool {
        self.accepted_ids()
            .iter()
            .any(|accepted_id| accepted_id.as_ref() == model_id)
    }

    pub fn has_valid_host_model_id(&self) -> bool {
        let host_model_id = self.effective_host_model_id();
        host_model_id
            .strip_prefix("MODEL_PLACEHOLDER_M")
            .and_then(|value| value.parse::<u16>().ok())
            .is_some_and(|value| (400..600).contains(&value))
    }
}

fn stable_hash(value: &str) -> u16 {
    let mut hash = 0x811c9dc5_u32;
    for byte in value.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    (hash % 200) as u16
}
