use super::{model::ReasoningLevel, provider::ParameterOverrides, ModelModality};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

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

pub(crate) fn input_modality_for_mime_type(mime_type: &str) -> ModelModality {
    let normalized = mime_type.trim().to_ascii_lowercase();
    if normalized.starts_with("image/") {
        ModelModality::Image
    } else if normalized.starts_with("audio/") || normalized.starts_with("video/audio/") {
        ModelModality::Audio
    } else if normalized.starts_with("video/") {
        ModelModality::Video
    } else {
        // 文本、代码、PDF 及其他文件输入统一归为文档，由协议适配器尽量转发。
        ModelModality::Document
    }
}

pub(crate) fn inline_data_filename(mime_type: &str) -> &'static str {
    let normalized = mime_type.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "application/pdf" => "input.pdf",
        "application/json" => "input.json",
        "application/rtf" | "text/rtf" => "input.rtf",
        "application/x-ipynb+json" => "input.ipynb",
        "application/x-javascript" | "text/javascript" => "input.js",
        "application/x-python-code" | "text/x-python" | "text/x-python-script" => "input.py",
        "application/x-typescript" | "text/x-typescript" => "input.ts",
        "text/css" => "input.css",
        "text/csv" => "input.csv",
        "text/html" => "input.html",
        "text/markdown" => "input.md",
        "text/plain" => "input.txt",
        "text/xml" => "input.xml",
        "audio/wav" | "audio/x-wav" => "input.wav",
        "audio/mpeg" | "audio/mp3" => "input.mp3",
        "audio/webm" | "audio/webm;codecs=opus" => "input.webm",
        "audio/flac" => "input.flac",
        "video/audio/s16le" => "input.pcm",
        "video/audio/wav" => "input.wav",
        "video/jpeg2000" | "video/videoframe/jpeg2000" => "input.j2k",
        "video/mp4" => "input.mp4",
        "video/text/timestamp" => "input.txt",
        "video/webm" => "input.webm",
        _ => "input.bin",
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct NeutralChatRequest {
    pub virtual_model_id: String,
    pub messages: Vec<NeutralMessage>,
    pub system_instruction: Option<String>,
    pub tools: Vec<NeutralTool>,
    /// 宿主本次请求期望的输出模态；为空时由具体协议和模型角色决定。
    pub output_modalities: BTreeSet<ModelModality>,
    /// 生图参数保持中立 JSON，由支持生图的协议适配器负责转换。
    pub image_generation_config: Option<serde_json::Value>,
    pub reasoning_level: Option<ReasoningLevel>,
    pub stream: bool,
    pub generation_parameters: ParameterOverrides,
    pub extra_body: HashMap<String, serde_json::Value>,
}
