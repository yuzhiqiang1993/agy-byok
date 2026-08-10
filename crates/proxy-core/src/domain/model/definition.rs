use super::checkpoint::ModelCompressionPolicy;
use super::reasoning::{ReasoningCapability, ReasoningLevel};
use super::token_limits::ModelTokenLimits;
use super::tokenizer::TokenizerConfig;
use crate::domain::provider::ParameterOverrides;
use crate::domain::serde_helpers::required_nullable;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub tools: bool,
    /// 上游模型可接收的内联数据 MIME。
    pub supported_mime_types: Vec<String>,
    pub reasoning: ReasoningCapability,
}

impl ModelCapabilities {
    pub fn effective_supported_mime_types(&self) -> BTreeSet<String> {
        self.supported_mime_types
            .iter()
            .map(|mime_type| mime_type.trim().to_ascii_lowercase())
            .filter(|mime_type| !mime_type.is_empty())
            .collect::<BTreeSet<_>>()
    }

    pub fn supports_video(&self) -> bool {
        self.effective_supported_mime_types()
            .iter()
            .any(|mime_type| mime_type.starts_with("video/"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamModel {
    pub id: String,
    pub provider_id: String,
    pub upstream_model_id: String,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
    pub token_limits: ModelTokenLimits,
    #[serde(deserialize_with = "required_nullable")]
    pub compression_policy: Option<ModelCompressionPolicy>,
    #[serde(deserialize_with = "required_nullable")]
    pub tokenizer: Option<TokenizerConfig>,
    pub parameter_overrides: ParameterOverrides,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualModel {
    pub id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub host_model_id: Option<String>,
    pub upstream_model_id: String,
    pub display_name: String,
    #[serde(deserialize_with = "required_nullable")]
    pub default_reasoning_level: Option<ReasoningLevel>,
    pub parameter_overrides: ParameterOverrides,
    #[serde(deserialize_with = "required_nullable")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capabilities_require_mime_types() {
        let error = serde_json::from_value::<ModelCapabilities>(json!({
            "vision": true,
            "tools": true,
            "reasoning": {
                "supported": null,
                "thinking_budget": null,
                "min_thinking_budget": null,
                "levels": {}
            }
        }))
        .unwrap_err();

        assert!(error.to_string().contains("supported_mime_types"));
    }

    #[test]
    fn reasoning_requires_explicit_nullable_metadata() {
        let error = serde_json::from_value::<ModelCapabilities>(json!({
            "vision": false,
            "tools": true,
            "supported_mime_types": [],
            "reasoning": { "levels": {} }
        }))
        .unwrap_err();

        assert!(error.to_string().contains("supported"));
    }
}
