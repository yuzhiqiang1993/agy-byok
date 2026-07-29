use super::request::NeutralContentBlock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    MaxTokens,
    ToolCall,
    ContentFilter,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChoice {
    pub index: u32,
    pub blocks: Vec<NeutralContentBlock>,
    pub finish_reason: Option<FinishReason>,
    pub raw_finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<NeutralChoice>,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NeutralStreamEvent {
    ResponseStart {
        response_id: Option<String>,
        model: String,
    },
    TextDelta {
        choice_index: u32,
        text: String,
    },
    ThinkingDelta {
        choice_index: u32,
        text: String,
    },
    ToolCallStart {
        choice_index: u32,
        tool_call_index: u32,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        choice_index: u32,
        tool_call_index: u32,
        arguments_delta: String,
    },
    ToolCallEnd {
        choice_index: u32,
        tool_call_index: u32,
    },
    UsageUpdate(UsageInfo),
    Finish {
        choice_index: u32,
        reason: FinishReason,
        raw_finish_reason: Option<String>,
    },
    ResponseEnd,
    Error {
        message: String,
        code: u16,
    },
}
