use super::{model::ReasoningLevel, provider::ParameterOverrides};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(crate) fn is_supported_inline_image_mime_type(mime_type: &str) -> bool {
    ["image/png", "image/jpeg", "image/webp"]
        .iter()
        .any(|supported| mime_type.eq_ignore_ascii_case(supported))
}

pub(crate) fn openai_input_audio_format(mime_type: &str) -> Option<&'static str> {
    if mime_type.eq_ignore_ascii_case("audio/wav") || mime_type.eq_ignore_ascii_case("audio/x-wav")
    {
        Some("wav")
    } else if mime_type.eq_ignore_ascii_case("audio/mpeg")
        || mime_type.eq_ignore_ascii_case("audio/mp3")
    {
        Some("mp3")
    } else {
        None
    }
}

pub(crate) fn is_supported_inline_document_mime_type(mime_type: &str) -> bool {
    mime_type.eq_ignore_ascii_case("application/pdf")
}

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
    /// Antigravity/Gemini 的内联二进制内容；具体协议适配器负责校验 MIME 支持范围。
    InlineData {
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
