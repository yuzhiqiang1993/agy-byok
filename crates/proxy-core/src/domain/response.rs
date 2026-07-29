use super::request::NeutralContentBlock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChatResponse {
    pub id: String,
    pub model: String,
    pub choices_blocks: Vec<NeutralContentBlock>,
    pub usage: Option<UsageInfo>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NeutralStreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallDelta {
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    UsageUpdate(UsageInfo),
    Finish {
        reason: String,
    },
    Error {
        message: String,
        code: u16,
    },
}
