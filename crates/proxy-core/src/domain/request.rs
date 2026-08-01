use super::{model::ReasoningLevel, provider::ParameterOverrides};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NeutralContentBlock {
    Text(String),
    Image {
        mime_type: String,
        data_base64: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
    },
    ToolResult {
        tool_call_id: String,
        #[serde(default)]
        name: Option<String>,
        content: String,
    },
    Thinking {
        text: String,
        signature: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralMessage {
    pub role: MessageRole,
    pub blocks: Vec<NeutralContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralToolFunction {
    pub name: String,
    pub description: Option<String>,
    pub parameters_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralTool {
    pub function: NeutralToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChatRequest {
    pub virtual_model_id: String,
    pub messages: Vec<NeutralMessage>,
    pub system_instruction: Option<String>,
    pub tools: Vec<NeutralTool>,
    pub reasoning_level: Option<ReasoningLevel>,
    pub stream: bool,
    pub generation_parameters: ParameterOverrides,
    pub extra_body: HashMap<String, serde_json::Value>,
}
