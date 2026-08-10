use crate::domain::serde_helpers::required_nullable;
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
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReasoningMapping {
    Disabled,
    Effort(String),
    BudgetTokens(u32),
    Adaptive,
    NativeLevel(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningCapability {
    /// 目录显式声明的思考能力；空值时由等级和预算推导。
    #[serde(deserialize_with = "required_nullable")]
    pub supported: Option<bool>,
    /// 宿主模型目录使用的默认思考预算，不直接替代具体推理等级的协议映射。
    #[serde(deserialize_with = "required_nullable")]
    pub thinking_budget: Option<i32>,
    /// 宿主允许为该模型设置的最小思考预算。
    #[serde(deserialize_with = "required_nullable")]
    pub min_thinking_budget: Option<u32>,
    pub levels: BTreeMap<ReasoningLevel, ReasoningMapping>,
}

impl ReasoningCapability {
    pub fn mapping_for(&self, level: ReasoningLevel) -> Option<&ReasoningMapping> {
        self.levels.get(&level)
    }

    pub fn supports_reasoning(&self) -> bool {
        self.supported.unwrap_or_else(|| {
            !self.levels.is_empty()
                || self.thinking_budget.is_some()
                || self.min_thinking_budget.is_some()
        })
    }
}
