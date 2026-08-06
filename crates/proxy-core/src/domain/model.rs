use super::provider::ParameterOverrides;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

const GEMINI_CONTEXT_WINDOW_LIMIT: u32 = 1_048_576;
const GEMINI_SAFE_TOKEN_THRESHOLD: u32 = 430_000;
const GEMINI_SAFE_MAX_TOKEN_LIMIT: u32 = 512_000;
const GEMINI_BALANCED_TOKEN_THRESHOLD: u32 = 640_000;
const GEMINI_BALANCED_MAX_TOKEN_LIMIT: u32 = 768_000;
const GEMINI_AGGRESSIVE_TOKEN_THRESHOLD: u32 = 760_000;
const GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT: u32 = 900_000;
const DEFAULT_GEMINI_MAX_OUTPUT_TOKENS: u32 = 16_384;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OfficialCompressionProfile {
    #[default]
    Official,
    Safe,
    Balanced,
    Aggressive,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OfficialModelSettings {
    /// 是否沿用官方模型目录中的检查点配置；其他档位会覆盖官方 Gemini 条目。
    pub gemini_compression_profile: OfficialCompressionProfile,
    /// 自定义 Provider 模型的 Checkpoint 压缩阈值百分比；缺省时沿用档位自动适配。
    #[serde(default)]
    pub custom_model_threshold_percent: Option<u8>,
    pub gemini_token_threshold: u32,
    pub gemini_max_token_limit: u32,
    pub gemini_max_output_tokens: u32,
}

impl Default for OfficialModelSettings {
    fn default() -> Self {
        Self {
            gemini_compression_profile: OfficialCompressionProfile::Official,
            custom_model_threshold_percent: None,
            gemini_token_threshold: GEMINI_BALANCED_TOKEN_THRESHOLD,
            gemini_max_token_limit: GEMINI_BALANCED_MAX_TOKEN_LIMIT,
            gemini_max_output_tokens: DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
        }
    }
}

impl OfficialModelSettings {
    /// 返回需要写入 Antigravity 模型目录的检查点参数；官方档位不覆盖上游值。
    pub fn gemini_checkpoint_limits(&self) -> Option<(u32, u32, u32)> {
        let (threshold, max_limit) = match self.gemini_compression_profile {
            OfficialCompressionProfile::Official => return None,
            OfficialCompressionProfile::Safe => {
                (GEMINI_SAFE_TOKEN_THRESHOLD, GEMINI_SAFE_MAX_TOKEN_LIMIT)
            }
            OfficialCompressionProfile::Balanced => (
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
            ),
            OfficialCompressionProfile::Aggressive => (
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
            ),
            OfficialCompressionProfile::Custom => {
                (self.gemini_token_threshold, self.gemini_max_token_limit)
            }
        };
        Some((threshold, max_limit, self.gemini_max_output_tokens))
    }

    /// 为自定义 Provider 模型生成按模型能力裁剪后的 Checkpoint 参数。
    ///
    /// `official` 只表示官方 Gemini 目录不覆盖上游值；自定义模型仍需要
    /// 一套本地 Checkpoint 配置，否则 Antigravity 会使用自己的默认策略。
    pub fn custom_model_checkpoint_limits(
        &self,
        input_token_limit: u32,
        output_token_limit: u32,
    ) -> Option<(u32, u32, u32)> {
        let (threshold, max_limit, max_output) = match self.gemini_compression_profile {
            OfficialCompressionProfile::Official => (
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Safe => (
                GEMINI_SAFE_TOKEN_THRESHOLD,
                GEMINI_SAFE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Balanced => (
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Aggressive => (
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Custom => (
                self.gemini_token_threshold,
                self.gemini_max_token_limit,
                self.gemini_max_output_tokens,
            ),
        };
        let max_limit = max_limit.min(input_token_limit);
        let max_output = max_output
            .min(output_token_limit)
            .min(max_limit.saturating_sub(1));
        let threshold = self
            .custom_model_threshold_percent
            .map(|percent| (u64::from(max_limit) * u64::from(percent) / 100) as u32)
            .unwrap_or(threshold);
        let threshold = threshold.min(max_limit.saturating_sub(max_output));
        (threshold > 0 && max_output > 0 && threshold < max_limit)
            .then_some((threshold, max_limit, max_output))
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(percent) = self.custom_model_threshold_percent {
            if percent == 0 || percent > 100 {
                return Err("自定义模型压缩阈值百分比必须在 1 到 100 之间".to_string());
            }
        }
        let Some((threshold, max_limit, max_output)) = self.gemini_checkpoint_limits() else {
            return Ok(());
        };
        if threshold == 0 || max_limit == 0 || max_output == 0 {
            return Err("官方 Gemini 检查点限制必须大于 0".to_string());
        }
        if threshold >= max_limit {
            return Err("官方 Gemini 压缩阈值必须小于检查点硬上限".to_string());
        }
        if max_limit > GEMINI_CONTEXT_WINDOW_LIMIT {
            return Err(format!(
                "官方 Gemini 检查点硬上限不能超过 {}",
                GEMINI_CONTEXT_WINDOW_LIMIT
            ));
        }
        if max_output >= max_limit {
            return Err("官方 Gemini 摘要输出预留必须小于检查点硬上限".to_string());
        }
        Ok(())
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelTokenLimits {
    /// 模型的上下文窗口；它独立于输入 Token 上限和 Checkpoint 压缩阈值。
    #[serde(default)]
    pub context_window: Option<u32>,
    /// 模型允许的最大输入 Token；缺省时由 Antigravity 适配层使用经验默认值。
    #[serde(default)]
    pub input_token_limit: Option<u32>,
    /// 模型允许的最大输出 Token；它独立于请求参数中的单次输出覆盖值。
    #[serde(default)]
    pub output_token_limit: Option<u32>,
}

impl ModelTokenLimits {
    pub const fn legacy_default() -> Self {
        Self {
            context_window: Some(128_000),
            input_token_limit: Some(128_000),
            output_token_limit: Some(8_192),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.context_window == Some(0) {
            return Err("context_window must be greater than 0");
        }
        if self.input_token_limit == Some(0) {
            return Err("input_token_limit must be greater than 0");
        }
        if self.output_token_limit == Some(0) {
            return Err("output_token_limit must be greater than 0");
        }
        Ok(())
    }
}

fn legacy_model_token_limits() -> ModelTokenLimits {
    ModelTokenLimits::legacy_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamModel {
    pub id: String,
    pub provider_id: String,
    pub upstream_model_id: String,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
    #[serde(default = "legacy_model_token_limits")]
    pub token_limits: ModelTokenLimits,
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
