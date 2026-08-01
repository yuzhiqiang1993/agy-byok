use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage,
    NeutralTool, NeutralToolFunction, ParameterOverrides, ProxyError,
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

pub struct AntigravityRequestParser;

impl AntigravityRequestParser {
    pub fn extract_model_id(body: &str) -> Result<String, ProxyError> {
        let val: Value = serde_json::from_str(body).map_err(|error| {
            ProxyError::new(
                ErrorCategory::InvalidRequest,
                format!("Failed to parse Antigravity JSON request: {error}"),
                400,
            )
        })?;
        Self::model_id_from_object(&val)
            .or_else(|| val.get("request").and_then(Self::model_id_from_object))
            .map(|model_id| {
                model_id
                    .strip_prefix("models/")
                    .unwrap_or(model_id)
                    .to_owned()
            })
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::InvalidRequest,
                    "Missing model ID in Antigravity request",
                    400,
                )
            })
    }

    fn model_id_from_object(value: &Value) -> Option<&str> {
        [
            "model",
            "requestedModel",
            "planModel",
            "requested_model",
            "plan_model",
            "modelId",
            "model_id",
        ]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
    }

    pub fn parse(body: &str) -> Result<NeutralChatRequest, ProxyError> {
        let val: Value = serde_json::from_str(body).map_err(|e| {
            ProxyError::new(
                ErrorCategory::InvalidRequest,
                format!("Failed to parse Antigravity JSON request: {}", e),
                400,
            )
        })?;
        let request_payload = val
            .get("request")
            .filter(|request| request.is_object())
            .unwrap_or(&val);

        let virtual_model_id = Self::extract_model_id(body)?;

        let stream = val["stream"]
            .as_bool()
            .or_else(|| request_payload["stream"].as_bool())
            .unwrap_or(true);
        let system_instruction = request_payload["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .or_else(|| request_payload["system_instruction"].as_str())
            .or_else(|| val["systemInstruction"]["parts"][0]["text"].as_str())
            .or_else(|| val["system_instruction"].as_str())
            .map(|s| s.to_string());

        let mut messages = Vec::new();
        let mut pending_tool_calls: HashMap<String, VecDeque<String>> = HashMap::new();
        if let Some(contents) = request_payload["contents"]
            .as_array()
            .or_else(|| request_payload["messages"].as_array())
            .or_else(|| val["contents"].as_array())
            .or_else(|| val["messages"].as_array())
        {
            for (message_index, item) in contents.iter().enumerate() {
                let role_str = item["role"].as_str().unwrap_or("user");
                let role = match role_str {
                    "user" => MessageRole::User,
                    "model" | "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "function" | "tool" => MessageRole::Tool,
                    _ => MessageRole::User,
                };

                let mut blocks = Vec::new();
                let mut tool_results = Vec::new();
                if let Some(parts) = item["parts"].as_array() {
                    for (part_index, part) in parts.iter().enumerate() {
                        if part["thought"].as_bool().unwrap_or(false) {
                            let text = part["text"].as_str().unwrap_or_default();
                            let signature = part["thoughtSignature"]
                                .as_str()
                                .or_else(|| part["thought_signature"].as_str())
                                .map(str::to_string);
                            if !text.is_empty() || signature.is_some() {
                                if let Some(NeutralContentBlock::Thinking {
                                    text: pending_text,
                                    signature: pending_signature,
                                }) = blocks.last_mut()
                                {
                                    pending_text.push_str(text);
                                    if signature.is_some() {
                                        *pending_signature = signature;
                                    }
                                } else {
                                    blocks.push(NeutralContentBlock::Thinking {
                                        text: text.to_string(),
                                        signature,
                                    });
                                }
                            }
                        } else if let Some(text) = part["text"].as_str() {
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
                            let id = fc["id"]
                                .as_str()
                                .filter(|id| !id.is_empty())
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("call_{message_index}_{part_index}"));
                            let arguments_json = match fc.get("args") {
                                Some(Value::String(arguments)) => arguments.clone(),
                                Some(arguments) => arguments.to_string(),
                                None => "{}".to_string(),
                            };
                            pending_tool_calls
                                .entry(name.clone())
                                .or_default()
                                .push_back(id.clone());
                            blocks.push(NeutralContentBlock::ToolCall {
                                id,
                                name,
                                arguments_json,
                            });
                        } else if let Some(fr) = part.get("functionResponse") {
                            let name = fr["name"].as_str().unwrap_or_default().to_string();
                            let explicit_id = fr["id"]
                                .as_str()
                                .filter(|id| !id.is_empty())
                                .map(str::to_string);
                            let id = if let Some(explicit_id) = explicit_id {
                                if let Some(queue) = pending_tool_calls.get_mut(&name) {
                                    if let Some(position) =
                                        queue.iter().position(|id| id == &explicit_id)
                                    {
                                        queue.remove(position);
                                    }
                                }
                                explicit_id
                            } else {
                                pending_tool_calls
                                    .get_mut(&name)
                                    .and_then(VecDeque::pop_front)
                                    .unwrap_or_else(|| format!("call_{name}"))
                            };
                            let content = match fr.get("response") {
                                Some(Value::String(response)) => response.clone(),
                                Some(response) => response.to_string(),
                                None => "{}".to_string(),
                            };
                            tool_results.push(NeutralMessage {
                                role: MessageRole::Tool,
                                blocks: vec![NeutralContentBlock::ToolResult {
                                    tool_call_id: id,
                                    name: (!name.is_empty()).then_some(name),
                                    content,
                                }],
                            });
                        }
                    }
                }

                if !blocks.is_empty() || tool_results.is_empty() {
                    messages.push(NeutralMessage { role, blocks });
                }
                messages.extend(tool_results);
            }
        }

        let mut tools = Vec::new();
        if let Some(tool_arr) = request_payload["tools"]
            .as_array()
            .or_else(|| val["tools"].as_array())
        {
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

        let gen_config = request_payload
            .get("generationConfig")
            .filter(|config| config.is_object())
            .unwrap_or(&val["generationConfig"]);
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
            reasoning_level: None,
            stream,
            generation_parameters,
            extra_body: std::collections::HashMap::new(),
        })
    }
}
