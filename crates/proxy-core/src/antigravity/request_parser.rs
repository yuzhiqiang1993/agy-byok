use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage,
    NeutralTool, NeutralToolFunction, ParameterOverrides, ProxyError,
};
use serde_json::Value;

pub struct AntigravityRequestParser;

impl AntigravityRequestParser {
    pub fn parse(body: &str) -> Result<NeutralChatRequest, ProxyError> {
        let val: Value = serde_json::from_str(body).map_err(|e| {
            ProxyError::new(
                ErrorCategory::InvalidRequest,
                format!("Failed to parse Antigravity JSON request: {}", e),
                400,
            )
        })?;

        // 从多种可能位置查找 virtual_model_id
        let virtual_model_id = val["model"]
            .as_str()
            .or_else(|| val["requestedModel"].as_str())
            .or_else(|| val["planModel"].as_str())
            .or_else(|| val["request"]["model"].as_str())
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::InvalidRequest,
                    "Missing model ID in Antigravity request",
                    400,
                )
            })?
            .to_string();

        let stream = val["stream"].as_bool().unwrap_or(true);
        let system_instruction = val["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .or_else(|| val["system_instruction"].as_str())
            .map(|s| s.to_string());

        let mut messages = Vec::new();
        if let Some(contents) = val["contents"]
            .as_array()
            .or_else(|| val["messages"].as_array())
        {
            for item in contents {
                let role_str = item["role"].as_str().unwrap_or("user");
                let role = match role_str {
                    "user" => MessageRole::User,
                    "model" | "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "function" | "tool" => MessageRole::Tool,
                    _ => MessageRole::User,
                };

                let mut blocks = Vec::new();
                if let Some(parts) = item["parts"].as_array() {
                    for part in parts {
                        if let Some(text) = part["text"].as_str() {
                            blocks.push(NeutralContentBlock::Text(text.to_string()));
                        } else if let Some(inline) = part.get("inlineData") {
                            let mime = inline["mimeType"].as_str().unwrap_or("image/png");
                            let data = inline["data"].as_str().unwrap_or_default();
                            blocks.push(NeutralContentBlock::Image {
                                mime_type: mime.to_string(),
                                data_base64: data.to_string(),
                            });
                        } else if let Some(fc) = part.get("functionCall") {
                            let name = fc["name"].as_str().unwrap_or_default().to_string();
                            let args = fc["args"].to_string();
                            blocks.push(NeutralContentBlock::ToolCall {
                                id: format!("call_{}", name),
                                name,
                                arguments_json: args,
                            });
                        } else if let Some(fr) = part.get("functionResponse") {
                            let id = fr["name"].as_str().unwrap_or_default().to_string();
                            let resp = fr["response"].to_string();
                            blocks.push(NeutralContentBlock::ToolResult {
                                tool_call_id: id,
                                content: resp,
                            });
                        }
                    }
                }

                messages.push(NeutralMessage { role, blocks });
            }
        }

        let mut tools = Vec::new();
        if let Some(tool_arr) = val["tools"].as_array() {
            for t in tool_arr {
                if let Some(decls) = t["functionDeclarations"].as_array() {
                    for decl in decls {
                        let name = decl["name"].as_str().unwrap_or_default().to_string();
                        let desc = decl["description"].as_str().map(|s| s.to_string());
                        let params = decl["parameters"].clone();
                        tools.push(NeutralTool {
                            function: NeutralToolFunction {
                                name,
                                description: desc,
                                parameters_schema: params,
                            },
                        });
                    }
                }
            }
        }

        let gen_config = &val["generationConfig"];
        let generation_parameters = ParameterOverrides {
            temperature: gen_config["temperature"].as_f64().map(|v| v as f32),
            max_tokens: gen_config["maxOutputTokens"].as_u64().map(|v| v as u32),
            top_p: gen_config["topP"].as_f64().map(|v| v as f32),
            top_k: gen_config["topK"].as_u64().map(|v| v as u32),
            extra_body: None,
        };

        Ok(NeutralChatRequest {
            virtual_model_id,
            messages,
            system_instruction,
            tools,
            stream,
            generation_parameters,
            extra_body: std::collections::HashMap::new(),
        })
    }
}
