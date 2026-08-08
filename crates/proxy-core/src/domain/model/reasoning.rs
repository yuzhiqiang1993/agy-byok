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
