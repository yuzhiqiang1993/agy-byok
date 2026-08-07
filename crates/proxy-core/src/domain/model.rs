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
const DEFAULT_TOKEN_THRESHOLD_PERCENT: u8 = 61;
const DEFAULT_MAX_TOKEN_LIMIT_PERCENT: u8 = 73;
const DEFAULT_MAX_OUTPUT_TOKENS_PERCENT: u8 = 2;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeCompressionProfile {
    #[default]
    Official,
    Safe,
    Balanced,
    Aggressive,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomModelCompressionProfile {
    Safe,
    Balanced,
    Aggressive,
    Custom,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClaudeCheckpointMetadata {
    pub capacity: u32,
    pub output_token_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelCheckpointOverride {
    Percentage {
        threshold_percent: u8,
    },
    Custom {
        token_threshold: u32,
        max_token_limit: u32,
        max_output_tokens: u32,
    },
}

impl ModelCheckpointOverride {
    pub fn validate(&self) -> Result<(), &'static str> {
        match *self {
            Self::Percentage { threshold_percent } => {
                if !(1..=100).contains(&threshold_percent) {
                    return Err("checkpoint threshold_percent must be between 1 and 100");
                }
            }
            Self::Custom {
                token_threshold,
                max_token_limit,
                max_output_tokens,
            } => {
                if token_threshold == 0 || max_token_limit == 0 || max_output_tokens == 0 {
                    return Err("custom checkpoint limits must be greater than 0");
                }
                if token_threshold >= max_token_limit {
                    return Err("checkpoint token_threshold must be less than max_token_limit");
                }
                if max_output_tokens >= max_token_limit {
                    return Err("checkpoint max_output_tokens must be less than max_token_limit");
                }
                if u64::from(token_threshold) + u64::from(max_output_tokens)
                    > u64::from(max_token_limit)
                {
                    return Err(
                        "checkpoint token_threshold plus max_output_tokens must not exceed max_token_limit",
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OfficialModelSettings {
    /// 是否沿用官方模型目录中的检查点配置；其他档位会覆盖官方 Gemini 条目。
    pub gemini_compression_profile: OfficialCompressionProfile,
    pub gemini_token_threshold_percent: u8,
    pub gemini_max_token_limit_percent: u8,
    pub gemini_max_output_tokens_percent: u8,
    pub claude_compression_profile: ClaudeCompressionProfile,
    pub claude_token_threshold_percent: u8,
    pub claude_max_token_limit_percent: u8,
    pub claude_max_output_tokens_percent: u8,
    pub custom_model_compression_profile: CustomModelCompressionProfile,
    pub custom_model_token_threshold_percent: u8,
    pub custom_model_max_token_limit_percent: u8,
    pub custom_model_max_output_tokens_percent: u8,
}

impl Default for OfficialModelSettings {
    fn default() -> Self {
        Self {
            gemini_compression_profile: OfficialCompressionProfile::Official,
            gemini_token_threshold_percent: DEFAULT_TOKEN_THRESHOLD_PERCENT,
            gemini_max_token_limit_percent: DEFAULT_MAX_TOKEN_LIMIT_PERCENT,
            gemini_max_output_tokens_percent: DEFAULT_MAX_OUTPUT_TOKENS_PERCENT,
            claude_compression_profile: ClaudeCompressionProfile::Official,
            claude_token_threshold_percent: DEFAULT_TOKEN_THRESHOLD_PERCENT,
            claude_max_token_limit_percent: DEFAULT_MAX_TOKEN_LIMIT_PERCENT,
            claude_max_output_tokens_percent: DEFAULT_MAX_OUTPUT_TOKENS_PERCENT,
            custom_model_compression_profile: CustomModelCompressionProfile::Balanced,
            custom_model_token_threshold_percent: DEFAULT_TOKEN_THRESHOLD_PERCENT,
            custom_model_max_token_limit_percent: DEFAULT_MAX_TOKEN_LIMIT_PERCENT,
            custom_model_max_output_tokens_percent: DEFAULT_MAX_OUTPUT_TOKENS_PERCENT,
        }
    }
}

impl OfficialModelSettings {
    /// 返回需要写入 Antigravity 模型目录的检查点参数；官方档位不覆盖上游值。
    pub fn gemini_checkpoint_limits(&self) -> Option<(u32, u32, u32)> {
        let (requested_threshold, max_limit, max_output) = match self.gemini_compression_profile {
            OfficialCompressionProfile::Official => return None,
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
            OfficialCompressionProfile::Custom => scale_percentage_checkpoint_limits(
                GEMINI_CONTEXT_WINDOW_LIMIT,
                self.gemini_token_threshold_percent,
                self.gemini_max_token_limit_percent,
                self.gemini_max_output_tokens_percent,
            ),
        };
        let threshold = requested_threshold.min(max_limit.saturating_sub(max_output));
        Some((threshold, max_limit, max_output))
    }

    pub(crate) fn claude_checkpoint_limits(
        &self,
        metadata: ClaudeCheckpointMetadata,
    ) -> Option<(u32, u32, u32)> {
        if metadata.capacity == 0 {
            return None;
        }

        let preset_reference = match self.claude_compression_profile {
            ClaudeCompressionProfile::Official => return None,
            ClaudeCompressionProfile::Safe => {
                Some((GEMINI_SAFE_TOKEN_THRESHOLD, GEMINI_SAFE_MAX_TOKEN_LIMIT))
            }
            ClaudeCompressionProfile::Balanced => Some((
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
            )),
            ClaudeCompressionProfile::Aggressive => Some((
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
            )),
            ClaudeCompressionProfile::Custom => None,
        };
        let (requested_threshold, max_limit, mut max_output) =
            if let Some((reference_threshold, reference_max_limit)) = preset_reference {
                (
                    scale_and_cap(
                        metadata.capacity,
                        reference_threshold,
                        GEMINI_CONTEXT_WINDOW_LIMIT,
                        metadata.capacity,
                    ),
                    scale_and_cap(
                        metadata.capacity,
                        reference_max_limit,
                        GEMINI_CONTEXT_WINDOW_LIMIT,
                        metadata.capacity,
                    ),
                    scale_and_cap(
                        metadata.capacity,
                        DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
                        GEMINI_CONTEXT_WINDOW_LIMIT,
                        metadata.capacity,
                    )
                    .max(1),
                )
            } else {
                scale_percentage_checkpoint_limits(
                    metadata.capacity,
                    self.claude_token_threshold_percent,
                    self.claude_max_token_limit_percent,
                    self.claude_max_output_tokens_percent,
                )
            };
        if max_limit == 0 {
            return None;
        }
        if let Some(output_token_limit) = metadata.output_token_limit.filter(|value| *value > 0) {
            max_output = max_output.min(output_token_limit);
        }
        max_output = max_output.min(max_limit.saturating_sub(1));
        let threshold = requested_threshold.min(max_limit.saturating_sub(max_output));

        (threshold > 0 && max_output > 0 && threshold < max_limit)
            .then_some((threshold, max_limit, max_output))
    }

    /// 为自定义 Provider 模型生成按模型能力裁剪后的 Checkpoint 参数。
    pub fn custom_model_checkpoint_limits(
        &self,
        effective_token_limit: u32,
        output_token_limit: u32,
    ) -> Option<(u32, u32, u32)> {
        self.custom_model_checkpoint_limits_with_override(
            None,
            effective_token_limit,
            output_token_limit,
        )
    }

    pub fn custom_model_checkpoint_limits_with_override(
        &self,
        checkpoint_override: Option<&ModelCheckpointOverride>,
        effective_token_limit: u32,
        output_token_limit: u32,
    ) -> Option<(u32, u32, u32)> {
        if checkpoint_override
            .is_some_and(|checkpoint_override| checkpoint_override.validate().is_err())
        {
            return None;
        }

        let (threshold, max_limit, max_output) = match checkpoint_override {
            Some(ModelCheckpointOverride::Custom {
                token_threshold,
                max_token_limit,
                max_output_tokens,
            }) => (*token_threshold, *max_token_limit, *max_output_tokens),
            _ => self.custom_model_checkpoint_profile_limits(effective_token_limit),
        };
        let max_limit = max_limit.min(effective_token_limit);
        let max_output = max_output
            .min(output_token_limit)
            .min(max_limit.saturating_sub(1));
        let threshold = match checkpoint_override {
            Some(ModelCheckpointOverride::Percentage { threshold_percent }) => {
                (u64::from(max_limit) * u64::from(*threshold_percent) / 100) as u32
            }
            _ => threshold,
        };
        let threshold = threshold.min(max_limit.saturating_sub(max_output));
        (threshold > 0 && max_output > 0 && threshold < max_limit)
            .then_some((threshold, max_limit, max_output))
    }

    fn custom_model_checkpoint_profile_limits(
        &self,
        effective_token_limit: u32,
    ) -> (u32, u32, u32) {
        let (threshold, max_limit, max_output) = match self.custom_model_compression_profile {
            CustomModelCompressionProfile::Safe => (
                GEMINI_SAFE_TOKEN_THRESHOLD,
                GEMINI_SAFE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Balanced => (
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Aggressive => (
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Custom => {
                return scale_percentage_checkpoint_limits(
                    effective_token_limit,
                    self.custom_model_token_threshold_percent,
                    self.custom_model_max_token_limit_percent,
                    self.custom_model_max_output_tokens_percent,
                );
            }
        };

        (
            scale_and_cap(
                effective_token_limit,
                threshold,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
            scale_and_cap(
                effective_token_limit,
                max_limit,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
            scale_and_cap(
                effective_token_limit,
                max_output,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        for (scope, percentages) in [
            (
                "官方 Gemini",
                (
                    self.gemini_token_threshold_percent,
                    self.gemini_max_token_limit_percent,
                    self.gemini_max_output_tokens_percent,
                ),
            ),
            (
                "Claude",
                (
                    self.claude_token_threshold_percent,
                    self.claude_max_token_limit_percent,
                    self.claude_max_output_tokens_percent,
                ),
            ),
            (
                "自定义模型",
                (
                    self.custom_model_token_threshold_percent,
                    self.custom_model_max_token_limit_percent,
                    self.custom_model_max_output_tokens_percent,
                ),
            ),
        ] {
            validate_percentage_triplet(scope, percentages)?;
        }
        Ok(())
    }
}

fn scale_and_cap(value: u32, numerator: u32, denominator: u32, cap: u32) -> u32 {
    (u64::from(value) * u64::from(numerator) / u64::from(denominator)).min(u64::from(cap)) as u32
}

fn scale_percentage_checkpoint_limits(
    capacity: u32,
    threshold_percent: u8,
    max_limit_percent: u8,
    max_output_percent: u8,
) -> (u32, u32, u32) {
    let max_limit = scale_and_cap(capacity, u32::from(max_limit_percent), 100, capacity);
    let requested_threshold = scale_and_cap(capacity, u32::from(threshold_percent), 100, capacity);
    let max_output = scale_and_cap(capacity, u32::from(max_output_percent), 100, capacity)
        .min(max_limit.saturating_sub(1));
    let threshold = requested_threshold.min(max_limit.saturating_sub(max_output));
    (threshold, max_limit, max_output)
}

fn validate_percentage_triplet(
    scope: &str,
    (threshold_percent, max_limit_percent, max_output_percent): (u8, u8, u8),
) -> Result<(), String> {
    if [threshold_percent, max_limit_percent, max_output_percent]
        .into_iter()
        .any(|percent| !(1..=100).contains(&percent))
    {
        return Err(format!("{scope} 自定义压缩百分比必须在 1 到 100 之间"));
    }
    if threshold_percent >= max_limit_percent {
        return Err(format!("{scope} 自定义压缩触发百分比必须小于硬上限百分比"));
    }
    if max_output_percent >= max_limit_percent {
        return Err(format!("{scope} 自定义摘要预留百分比必须小于硬上限百分比"));
    }
    if u16::from(threshold_percent) + u16::from(max_output_percent) > u16::from(max_limit_percent) {
        return Err(format!(
            "{scope} 自定义压缩触发百分比与摘要预留百分比之和不能超过硬上限百分比"
        ));
    }
    Ok(())
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenLimitSource {
    Catalog,
    Configured,
    Estimated,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelTokenLimits {
    /// 模型的上下文窗口；它独立于输入 Token 上限和 Checkpoint 压缩阈值。
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub context_window_source: TokenLimitSource,
    /// 模型允许的最大输入 Token；缺省时由 Antigravity 适配层使用经验默认值。
    #[serde(default)]
    pub input_token_limit: Option<u32>,
    #[serde(default)]
    pub input_token_limit_source: TokenLimitSource,
    /// 模型允许的最大输出 Token；它独立于请求参数中的单次输出覆盖值。
    #[serde(default)]
    pub output_token_limit: Option<u32>,
    #[serde(default)]
    pub output_token_limit_source: TokenLimitSource,
}

impl ModelTokenLimits {
    pub const fn legacy_default() -> Self {
        Self {
            context_window: Some(128_000),
            context_window_source: TokenLimitSource::Estimated,
            input_token_limit: Some(128_000),
            input_token_limit_source: TokenLimitSource::Estimated,
            output_token_limit: Some(8_192),
            output_token_limit_source: TokenLimitSource::Estimated,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TiktokenEncoding {
    Cl100kBase,
    O200kBase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenizerConfig {
    Tiktoken { encoding: TiktokenEncoding },
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
    #[serde(default)]
    pub checkpoint_override: Option<ModelCheckpointOverride>,
    #[serde(default)]
    pub tokenizer: Option<TokenizerConfig>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_override_validation_enforces_contract() {
        assert!(ModelCheckpointOverride::Percentage {
            threshold_percent: 1,
        }
        .validate()
        .is_ok());
        assert!(ModelCheckpointOverride::Percentage {
            threshold_percent: 100,
        }
        .validate()
        .is_ok());
        assert!(ModelCheckpointOverride::Custom {
            token_threshold: 80,
            max_token_limit: 100,
            max_output_tokens: 20,
        }
        .validate()
        .is_ok());

        for invalid in [
            ModelCheckpointOverride::Percentage {
                threshold_percent: 0,
            },
            ModelCheckpointOverride::Percentage {
                threshold_percent: 101,
            },
            ModelCheckpointOverride::Custom {
                token_threshold: 0,
                max_token_limit: 100,
                max_output_tokens: 20,
            },
            ModelCheckpointOverride::Custom {
                token_threshold: 100,
                max_token_limit: 100,
                max_output_tokens: 1,
            },
            ModelCheckpointOverride::Custom {
                token_threshold: 1,
                max_token_limit: 100,
                max_output_tokens: 100,
            },
            ModelCheckpointOverride::Custom {
                token_threshold: 80,
                max_token_limit: 100,
                max_output_tokens: 30,
            },
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn official_model_settings_default_to_independent_profiles_and_percentages() {
        let settings = OfficialModelSettings::default();

        assert_eq!(
            settings.gemini_compression_profile,
            OfficialCompressionProfile::Official
        );
        assert_eq!(
            settings.claude_compression_profile,
            ClaudeCompressionProfile::Official
        );
        assert_eq!(
            settings.custom_model_compression_profile,
            CustomModelCompressionProfile::Balanced
        );
        assert_eq!(
            [
                settings.gemini_token_threshold_percent,
                settings.gemini_max_token_limit_percent,
                settings.gemini_max_output_tokens_percent,
                settings.claude_token_threshold_percent,
                settings.claude_max_token_limit_percent,
                settings.claude_max_output_tokens_percent,
                settings.custom_model_token_threshold_percent,
                settings.custom_model_max_token_limit_percent,
                settings.custom_model_max_output_tokens_percent,
            ],
            [61, 73, 2, 61, 73, 2, 61, 73, 2]
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits(200_000, 32_000),
            Some((122_070, 146_484, 3_125))
        );
    }

    #[test]
    fn compression_profiles_and_percentages_round_trip_with_snake_case_schema() {
        let settings = OfficialModelSettings {
            gemini_compression_profile: OfficialCompressionProfile::Custom,
            gemini_token_threshold_percent: 70,
            gemini_max_token_limit_percent: 90,
            gemini_max_output_tokens_percent: 5,
            claude_compression_profile: ClaudeCompressionProfile::Custom,
            claude_token_threshold_percent: 70,
            claude_max_token_limit_percent: 90,
            claude_max_output_tokens_percent: 5,
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 65,
            custom_model_max_token_limit_percent: 85,
            custom_model_max_output_tokens_percent: 4,
        };

        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["gemini_compression_profile"], "custom");
        assert_eq!(value["gemini_token_threshold_percent"], 70);
        assert_eq!(value["gemini_max_token_limit_percent"], 90);
        assert_eq!(value["gemini_max_output_tokens_percent"], 5);
        assert_eq!(value["claude_compression_profile"], "custom");
        assert_eq!(value["claude_token_threshold_percent"], 70);
        assert_eq!(value["claude_max_token_limit_percent"], 90);
        assert_eq!(value["claude_max_output_tokens_percent"], 5);
        assert_eq!(value["custom_model_compression_profile"], "custom");
        assert_eq!(value["custom_model_token_threshold_percent"], 65);
        assert_eq!(value["custom_model_max_token_limit_percent"], 85);
        assert_eq!(value["custom_model_max_output_tokens_percent"], 4);
        assert_eq!(value.as_object().unwrap().len(), 12);
        assert_eq!(
            serde_json::from_value::<OfficialModelSettings>(value).unwrap(),
            settings
        );
    }

    #[test]
    fn custom_claude_profile_scales_capacity_and_safely_clips_limits() {
        let metadata = ClaudeCheckpointMetadata {
            capacity: 200_000,
            output_token_limit: Some(32_000),
        };
        let defaults = OfficialModelSettings {
            claude_compression_profile: ClaudeCompressionProfile::Custom,
            ..OfficialModelSettings::default()
        };
        assert_eq!(
            defaults.claude_checkpoint_limits(metadata),
            Some((122_000, 146_000, 4_000))
        );

        let configured = OfficialModelSettings {
            claude_compression_profile: ClaudeCompressionProfile::Custom,
            claude_token_threshold_percent: 70,
            claude_max_token_limit_percent: 90,
            claude_max_output_tokens_percent: 5,
            ..OfficialModelSettings::default()
        };
        assert_eq!(
            configured.claude_checkpoint_limits(metadata),
            Some((140_000, 180_000, 10_000))
        );
        assert_eq!(
            configured.claude_checkpoint_limits(ClaudeCheckpointMetadata {
                output_token_limit: Some(8_000),
                ..metadata
            }),
            Some((140_000, 180_000, 8_000))
        );
        assert_eq!(
            configured.claude_checkpoint_limits(ClaudeCheckpointMetadata {
                capacity: 0,
                ..metadata
            }),
            None
        );
    }

    #[test]
    fn catalog_capacity_claude_presets_ignore_existing_checkpoint_values() {
        let metadata = ClaudeCheckpointMetadata {
            capacity: 200_000,
            output_token_limit: Some(32_000),
        };
        let settings = OfficialModelSettings {
            claude_compression_profile: ClaudeCompressionProfile::Safe,
            ..OfficialModelSettings::default()
        };

        assert_eq!(
            settings.claude_checkpoint_limits(metadata),
            Some((82_015, 97_656, 3_125))
        );
    }

    #[test]
    fn validates_claude_percentage_triplets() {
        assert!(OfficialModelSettings::default().validate().is_ok());

        for (threshold_percent, max_limit_percent, max_output_percent) in [
            (0, 73, 2),
            (101, 73, 2),
            (61, 0, 2),
            (61, 101, 2),
            (61, 73, 0),
            (61, 73, 101),
            (73, 73, 2),
            (61, 73, 73),
            (70, 73, 4),
        ] {
            let settings = OfficialModelSettings {
                claude_token_threshold_percent: threshold_percent,
                claude_max_token_limit_percent: max_limit_percent,
                claude_max_output_tokens_percent: max_output_percent,
                ..OfficialModelSettings::default()
            };
            assert!(
                settings.validate().is_err(),
                "unexpected valid Claude percentages: {threshold_percent}/{max_limit_percent}/{max_output_percent}"
            );
        }
    }

    #[test]
    fn gemini_presets_use_fixed_limits() {
        for (profile, expected) in [
            (
                OfficialCompressionProfile::Safe,
                (430_000, 512_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
            ),
            (
                OfficialCompressionProfile::Balanced,
                (640_000, 768_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
            ),
            (
                OfficialCompressionProfile::Aggressive,
                (760_000, 900_000, DEFAULT_GEMINI_MAX_OUTPUT_TOKENS),
            ),
        ] {
            let settings = OfficialModelSettings {
                gemini_compression_profile: profile,
                ..OfficialModelSettings::default()
            };

            assert_eq!(settings.gemini_checkpoint_limits(), Some(expected));
            assert!(settings.validate().is_ok());
        }
    }

    #[test]
    fn gemini_custom_percentages_scale_from_context_window() {
        let settings = OfficialModelSettings {
            gemini_compression_profile: OfficialCompressionProfile::Custom,
            gemini_token_threshold_percent: 70,
            gemini_max_token_limit_percent: 90,
            gemini_max_output_tokens_percent: 5,
            ..OfficialModelSettings::default()
        };

        assert_eq!(
            settings.gemini_checkpoint_limits(),
            Some((734_003, 943_718, 52_428))
        );
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn custom_model_custom_profile_scales_three_percentages() {
        let settings = OfficialModelSettings {
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 70,
            custom_model_max_token_limit_percent: 90,
            custom_model_max_output_tokens_percent: 5,
            ..OfficialModelSettings::default()
        };
        assert_eq!(
            settings.custom_model_checkpoint_limits(200_000, 32_000),
            Some((140_000, 180_000, 10_000))
        );
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn validates_gemini_and_custom_model_percentage_fields() {
        for (threshold, max_limit, max_output) in [
            (0, 90, 5),
            (101, 90, 5),
            (70, 0, 5),
            (70, 101, 5),
            (70, 90, 0),
            (70, 90, 101),
            (90, 90, 5),
            (70, 90, 90),
            (70, 73, 4),
        ] {
            let gemini = OfficialModelSettings {
                gemini_compression_profile: OfficialCompressionProfile::Custom,
                gemini_token_threshold_percent: threshold,
                gemini_max_token_limit_percent: max_limit,
                gemini_max_output_tokens_percent: max_output,
                ..OfficialModelSettings::default()
            };
            assert!(
                gemini.validate().is_err(),
                "unexpected valid Gemini percentages: {threshold}/{max_limit}/{max_output}"
            );

            let custom = OfficialModelSettings {
                custom_model_compression_profile: CustomModelCompressionProfile::Custom,
                custom_model_token_threshold_percent: threshold,
                custom_model_max_token_limit_percent: max_limit,
                custom_model_max_output_tokens_percent: max_output,
                ..OfficialModelSettings::default()
            };
            assert!(
                custom.validate().is_err(),
                "unexpected valid custom-model percentages: {threshold}/{max_limit}/{max_output}"
            );
        }

        let inactive_profiles_with_invalid_percentages = OfficialModelSettings {
            gemini_compression_profile: OfficialCompressionProfile::Balanced,
            gemini_token_threshold_percent: 0,
            gemini_max_token_limit_percent: 0,
            gemini_max_output_tokens_percent: 0,
            custom_model_compression_profile: CustomModelCompressionProfile::Balanced,
            custom_model_token_threshold_percent: 0,
            custom_model_max_token_limit_percent: 0,
            custom_model_max_output_tokens_percent: 0,
            ..OfficialModelSettings::default()
        };
        assert!(inactive_profiles_with_invalid_percentages
            .validate()
            .is_err());
    }

    #[test]
    fn custom_model_presets_scale_relative_to_effective_input_limit() {
        for (profile, expected) in [
            (CustomModelCompressionProfile::Safe, (82_015, 97_656, 3_125)),
            (
                CustomModelCompressionProfile::Balanced,
                (122_070, 146_484, 3_125),
            ),
            (
                CustomModelCompressionProfile::Aggressive,
                (144_958, 171_661, 3_125),
            ),
        ] {
            let settings = OfficialModelSettings {
                custom_model_compression_profile: profile,
                ..OfficialModelSettings::default()
            };

            assert_eq!(
                settings.custom_model_checkpoint_limits(200_000, 32_000),
                Some(expected)
            );
        }
    }

    #[test]
    fn custom_model_profile_preserves_checkpoint_override_priority() {
        let settings = OfficialModelSettings {
            custom_model_compression_profile: CustomModelCompressionProfile::Balanced,
            ..OfficialModelSettings::default()
        };
        let percentage = ModelCheckpointOverride::Percentage {
            threshold_percent: 80,
        };
        let custom = ModelCheckpointOverride::Custom {
            token_threshold: 150_000,
            max_token_limit: 180_000,
            max_output_tokens: 10_000,
        };

        assert_eq!(
            settings.custom_model_checkpoint_limits(200_000, 32_000),
            Some((122_070, 146_484, 3_125))
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(
                Some(&percentage),
                200_000,
                32_000,
            ),
            Some((117_187, 146_484, 3_125))
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(Some(&custom), 200_000, 32_000,),
            Some((150_000, 180_000, 10_000))
        );
    }

    #[test]
    fn checkpoint_resolution_honors_override_priority_and_safety_clipping() {
        let settings = OfficialModelSettings {
            custom_model_compression_profile: CustomModelCompressionProfile::Custom,
            custom_model_token_threshold_percent: 60,
            custom_model_max_token_limit_percent: 80,
            custom_model_max_output_tokens_percent: 5,
            ..OfficialModelSettings::default()
        };
        let percentage = ModelCheckpointOverride::Percentage {
            threshold_percent: 80,
        };
        let custom = ModelCheckpointOverride::Custom {
            token_threshold: 250_000,
            max_token_limit: 300_000,
            max_output_tokens: 20_000,
        };

        assert_eq!(
            settings.custom_model_checkpoint_limits(372_000, 128_000),
            Some((223_200, 297_600, 18_600))
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(
                Some(&percentage),
                372_000,
                128_000,
            ),
            Some((238_080, 297_600, 18_600))
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(Some(&custom), 372_000, 128_000,),
            Some((250_000, 300_000, 20_000))
        );
        assert_eq!(
            settings.custom_model_checkpoint_limits_with_override(Some(&custom), 200_000, 10_000,),
            Some((190_000, 200_000, 10_000))
        );
    }
}
