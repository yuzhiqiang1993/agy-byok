use crate::domain::serde_helpers::required_nullable;
use serde::{Deserialize, Serialize};

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
#[serde(deny_unknown_fields)]
pub struct ModelTokenLimits {
    /// 模型的上下文窗口；它独立于输入 Token 上限和 Checkpoint 压缩阈值。
    #[serde(deserialize_with = "required_nullable")]
    pub context_window: Option<u32>,
    pub context_window_source: TokenLimitSource,
    /// 模型允许的最大输入 Token；缺省时由 Antigravity 适配层使用经验默认值。
    #[serde(deserialize_with = "required_nullable")]
    pub input_token_limit: Option<u32>,
    pub input_token_limit_source: TokenLimitSource,
    /// 模型允许的最大输出 Token；它独立于请求参数中的单次输出覆盖值。
    #[serde(deserialize_with = "required_nullable")]
    pub output_token_limit: Option<u32>,
    pub output_token_limit_source: TokenLimitSource,
}

impl ModelTokenLimits {
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
