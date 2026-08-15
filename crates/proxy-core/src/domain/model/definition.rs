use super::checkpoint::ModelCompressionPolicy;
use super::reasoning::{ReasoningCapability, ReasoningLevel};
use super::token_limits::ModelTokenLimits;
use super::tokenizer::TokenizerConfig;
use crate::domain::provider::ParameterOverrides;
use crate::domain::serde_helpers::required_nullable;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Agent,
    Command,
    Tab,
    ImageGeneration,
    Mquery,
    WebSearch,
    CommitMessage,
    AudioTranscription,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelModality {
    Text,
    Image,
    Audio,
    Video,
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub roles: BTreeSet<ModelRole>,
    pub input_modalities: BTreeSet<ModelModality>,
    pub output_modalities: BTreeSet<ModelModality>,
    pub tools: bool,
    /// 上游模型可接收的输入数据 MIME；输出能力由 output_modalities 单独声明。
    pub input_mime_types: Vec<String>,
    pub reasoning: ReasoningCapability,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            roles: BTreeSet::from([ModelRole::Agent]),
            input_modalities: BTreeSet::from([ModelModality::Text]),
            output_modalities: BTreeSet::from([ModelModality::Text]),
            tools: false,
            input_mime_types: Vec::new(),
            reasoning: ReasoningCapability::default(),
        }
    }
}

impl ModelCapabilities {
    pub fn effective_input_mime_types(&self) -> BTreeSet<String> {
        self.input_mime_types
            .iter()
            .map(|mime_type| mime_type.trim().to_ascii_lowercase())
            .filter(|mime_type| !mime_type.is_empty())
            .collect::<BTreeSet<_>>()
    }

    pub fn supports_input(&self, modality: ModelModality) -> bool {
        self.input_modalities.contains(&modality)
    }

    pub fn supports_output(&self, modality: ModelModality) -> bool {
        self.output_modalities.contains(&modality)
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
        let prefixed_id = if self.id.starts_with("custom-") {
            Cow::Borrowed(self.id.as_str())
        } else {
            Cow::Owned(format!("custom-{}", self.id))
        };
        if !prefixed_id.contains('_') {
            return prefixed_id;
        }

        // Antigravity 会转换模型映射的对象键，却不会同步转换排序表中的字符串引用。
        // 含下划线时改用稳定的宿主槽位，原始 ID 仍由 accepted_ids() 保留用于兼容路由。
        let host_model_id = self.effective_host_model_id();
        let slot = host_model_id
            .strip_prefix("MODEL_PLACEHOLDER_")
            .unwrap_or(host_model_id.as_ref());
        Cow::Owned(format!("custom-byok-{}", slot.to_ascii_lowercase()))
    }

    pub fn accepted_ids(&self) -> [Cow<'_, str>; 3] {
        [
            Cow::Borrowed(self.id.as_str()),
            self.effective_host_model_id(),
            self.catalog_key(),
        ]
    }

    pub fn matches_id(&self, model_id: &str) -> bool {
        let clean_id = model_id.strip_prefix("models/").unwrap_or(model_id);
        self.accepted_ids()
            .iter()
            .any(|accepted_id| accepted_id.as_ref() == clean_id || accepted_id.as_ref() == model_id)
    }

    pub fn has_valid_host_model_id(&self) -> bool {
        let host_id = self.effective_host_model_id();
        host_id.starts_with("MODEL_PLACEHOLDER_") && host_id.len() > "MODEL_PLACEHOLDER_".len()
    }
}

pub fn stable_hash(value: &str) -> u16 {
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
            "roles": ["agent"],
            "input_modalities": ["text", "image"],
            "output_modalities": ["text"],
            "tools": true,
            "reasoning": {
                "supported": null,
                "thinking_budget": null,
                "min_thinking_budget": null,
                "levels": {}
            }
        }))
        .unwrap_err();

        assert!(error.to_string().contains("input_mime_types"));
    }

    #[test]
    fn reasoning_requires_explicit_nullable_metadata() {
        let error = serde_json::from_value::<ModelCapabilities>(json!({
            "roles": ["agent"],
            "input_modalities": ["text"],
            "output_modalities": ["text"],
            "tools": true,
            "input_mime_types": [],
            "reasoning": { "levels": {} }
        }))
        .unwrap_err();

        assert!(error.to_string().contains("supported"));
    }
}
