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

use serde::{Deserialize, Serialize};

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
pub enum CustomModelCompressionProfile {
    #[default]
    None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckpointLimits {
    pub(crate) token_threshold: u32,
    pub(crate) max_token_limit: u32,
    pub(crate) max_output_tokens: u32,
}

impl CheckpointLimits {
    const fn new(token_threshold: u32, max_token_limit: u32, max_output_tokens: u32) -> Self {
        Self {
            token_threshold,
            max_token_limit,
            max_output_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompressionPercentages {
    pub token_threshold: u8,
    pub max_token_limit: u8,
    pub max_output_tokens: u8,
}

impl Default for CompressionPercentages {
    fn default() -> Self {
        Self {
            token_threshold: DEFAULT_TOKEN_THRESHOLD_PERCENT,
            max_token_limit: DEFAULT_MAX_TOKEN_LIMIT_PERCENT,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS_PERCENT,
        }
    }
}

impl CompressionPercentages {
    fn validate(self, scope: &str) -> Result<(), String> {
        if [
            self.token_threshold,
            self.max_token_limit,
            self.max_output_tokens,
        ]
        .into_iter()
        .any(|percent| !(1..=100).contains(&percent))
        {
            return Err(format!("{scope} 自定义压缩百分比必须在 1 到 100 之间"));
        }
        if self.token_threshold >= self.max_token_limit {
            return Err(format!("{scope} 自定义压缩触发百分比必须小于硬上限百分比"));
        }
        if self.max_output_tokens >= self.max_token_limit {
            return Err(format!("{scope} 自定义摘要预留百分比必须小于硬上限百分比"));
        }
        if u16::from(self.token_threshold) + u16::from(self.max_output_tokens)
            > u16::from(self.max_token_limit)
        {
            return Err(format!(
                "{scope} 自定义压缩触发百分比与摘要预留百分比之和不能超过硬上限百分比"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialCompressionSettings {
    pub profile: OfficialCompressionProfile,
    pub percentages: CompressionPercentages,
}

impl Default for OfficialCompressionSettings {
    fn default() -> Self {
        Self {
            profile: OfficialCompressionProfile::Official,
            percentages: CompressionPercentages::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustomModelCompressionSettings {
    pub profile: CustomModelCompressionProfile,
    pub percentages: CompressionPercentages,
}

impl Default for CustomModelCompressionSettings {
    fn default() -> Self {
        Self {
            profile: CustomModelCompressionProfile::None,
            percentages: CompressionPercentages::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialModelSettings {
    pub gemini: OfficialCompressionSettings,
    pub claude: OfficialCompressionSettings,
    pub custom_model: CustomModelCompressionSettings,
}

impl OfficialModelSettings {
    /// 返回需要写入 Antigravity 模型目录的检查点参数；官方档位不覆盖上游值。
    pub(crate) fn gemini_checkpoint_limits(&self) -> Option<CheckpointLimits> {
        let requested = match self.gemini.profile {
            OfficialCompressionProfile::Official => return None,
            OfficialCompressionProfile::Safe => CheckpointLimits::new(
                GEMINI_SAFE_TOKEN_THRESHOLD,
                GEMINI_SAFE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Balanced => CheckpointLimits::new(
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Aggressive => CheckpointLimits::new(
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            OfficialCompressionProfile::Custom => scale_percentage_checkpoint_limits(
                GEMINI_CONTEXT_WINDOW_LIMIT,
                self.gemini.percentages,
            ),
        };
        Some(CheckpointLimits::new(
            requested.token_threshold.min(
                requested
                    .max_token_limit
                    .saturating_sub(requested.max_output_tokens),
            ),
            requested.max_token_limit,
            requested.max_output_tokens,
        ))
    }

    pub(crate) fn claude_checkpoint_limits(
        &self,
        metadata: ClaudeCheckpointMetadata,
    ) -> Option<CheckpointLimits> {
        if metadata.capacity == 0 {
            return None;
        }

        let preset_reference = match self.claude.profile {
            OfficialCompressionProfile::Official => return None,
            OfficialCompressionProfile::Safe => {
                Some((GEMINI_SAFE_TOKEN_THRESHOLD, GEMINI_SAFE_MAX_TOKEN_LIMIT))
            }
            OfficialCompressionProfile::Balanced => Some((
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
            )),
            OfficialCompressionProfile::Aggressive => Some((
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
            )),
            OfficialCompressionProfile::Custom => None,
        };
        let mut limits = if let Some((reference_threshold, reference_max_limit)) = preset_reference
        {
            CheckpointLimits::new(
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
            scale_percentage_checkpoint_limits(metadata.capacity, self.claude.percentages)
        };
        if limits.max_token_limit == 0 {
            return None;
        }
        if let Some(output_token_limit) = metadata.output_token_limit.filter(|value| *value > 0) {
            limits.max_output_tokens = limits.max_output_tokens.min(output_token_limit);
        }
        limits.max_output_tokens = limits
            .max_output_tokens
            .min(limits.max_token_limit.saturating_sub(1));
        limits.token_threshold = limits.token_threshold.min(
            limits
                .max_token_limit
                .saturating_sub(limits.max_output_tokens),
        );

        (limits.token_threshold > 0
            && limits.max_output_tokens > 0
            && limits.token_threshold < limits.max_token_limit)
            .then_some(limits)
    }

    pub(crate) fn custom_model_checkpoint_limits_with_override(
        &self,
        checkpoint_override: Option<&ModelCheckpointOverride>,
        effective_token_limit: u32,
        output_token_limit: u32,
    ) -> Option<CheckpointLimits> {
        if checkpoint_override
            .is_some_and(|checkpoint_override| checkpoint_override.validate().is_err())
        {
            return None;
        }

        let requested = match checkpoint_override {
            Some(ModelCheckpointOverride::Custom {
                token_threshold,
                max_token_limit,
                max_output_tokens,
            }) => CheckpointLimits::new(*token_threshold, *max_token_limit, *max_output_tokens),
            Some(ModelCheckpointOverride::Percentage { .. }) => self
                .custom_model_checkpoint_profile_limits(effective_token_limit)
                .unwrap_or_else(|| {
                    // 模型级比例是显式开启策略；全局“不设置”时使用稳定的默认比例作为
                    // 硬上限和摘要预留基准，仅让模型级阈值比例覆盖触发点。
                    scale_percentage_checkpoint_limits(
                        effective_token_limit,
                        CompressionPercentages::default(),
                    )
                }),
            None => self.custom_model_checkpoint_profile_limits(effective_token_limit)?,
        };
        let max_token_limit = requested.max_token_limit.min(effective_token_limit);
        let max_output_tokens = requested
            .max_output_tokens
            .min(output_token_limit)
            .min(max_token_limit.saturating_sub(1));
        let token_threshold = match checkpoint_override {
            Some(ModelCheckpointOverride::Percentage { threshold_percent }) => {
                (u64::from(max_token_limit) * u64::from(*threshold_percent) / 100) as u32
            }
            _ => requested.token_threshold,
        };
        let token_threshold =
            token_threshold.min(max_token_limit.saturating_sub(max_output_tokens));
        (token_threshold > 0 && max_output_tokens > 0 && token_threshold < max_token_limit)
            .then_some(CheckpointLimits::new(
                token_threshold,
                max_token_limit,
                max_output_tokens,
            ))
    }

    fn custom_model_checkpoint_profile_limits(
        &self,
        effective_token_limit: u32,
    ) -> Option<CheckpointLimits> {
        let reference = match self.custom_model.profile {
            CustomModelCompressionProfile::None => return None,
            CustomModelCompressionProfile::Safe => CheckpointLimits::new(
                GEMINI_SAFE_TOKEN_THRESHOLD,
                GEMINI_SAFE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Balanced => CheckpointLimits::new(
                GEMINI_BALANCED_TOKEN_THRESHOLD,
                GEMINI_BALANCED_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Aggressive => CheckpointLimits::new(
                GEMINI_AGGRESSIVE_TOKEN_THRESHOLD,
                GEMINI_AGGRESSIVE_MAX_TOKEN_LIMIT,
                DEFAULT_GEMINI_MAX_OUTPUT_TOKENS,
            ),
            CustomModelCompressionProfile::Custom => {
                return scale_percentage_checkpoint_limits(
                    effective_token_limit,
                    self.custom_model.percentages,
                )
                .into();
            }
        };

        Some(CheckpointLimits::new(
            scale_and_cap(
                effective_token_limit,
                reference.token_threshold,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
            scale_and_cap(
                effective_token_limit,
                reference.max_token_limit,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
            scale_and_cap(
                effective_token_limit,
                reference.max_output_tokens,
                GEMINI_CONTEXT_WINDOW_LIMIT,
                effective_token_limit,
            ),
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.gemini.percentages.validate("官方 Gemini")?;
        self.claude.percentages.validate("Claude")?;
        self.custom_model.percentages.validate("自定义模型")?;
        Ok(())
    }
}

fn scale_and_cap(value: u32, numerator: u32, denominator: u32, cap: u32) -> u32 {
    (u64::from(value) * u64::from(numerator) / u64::from(denominator)).min(u64::from(cap)) as u32
}

fn scale_percentage_checkpoint_limits(
    capacity: u32,
    percentages: CompressionPercentages,
) -> CheckpointLimits {
    let max_token_limit = scale_and_cap(
        capacity,
        u32::from(percentages.max_token_limit),
        100,
        capacity,
    );
    let requested_threshold = scale_and_cap(
        capacity,
        u32::from(percentages.token_threshold),
        100,
        capacity,
    );
    let max_output_tokens = scale_and_cap(
        capacity,
        u32::from(percentages.max_output_tokens),
        100,
        capacity,
    )
    .min(max_token_limit.saturating_sub(1));
    CheckpointLimits::new(
        requested_threshold.min(max_token_limit.saturating_sub(max_output_tokens)),
        max_token_limit,
        max_output_tokens,
    )
}

#[cfg(test)]
mod tests;
