use crate::domain::model::ReasoningMapping;
use crate::domain::{
    inline_data_filename, input_modality_for_mime_type, openai_input_audio_format, ErrorCategory,
    MessageRole, ModelModality, NeutralChatRequest, NeutralContentBlock, NeutralMessage, Provider,
    ProxyError,
};
use crate::providers::is_image_generation_request;
use crate::routing::ResolvedRoute;
use serde_json::{json, Value};
use std::collections::HashMap;

fn normalize_json_schema_types(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(schema_type) = object.get_mut("type") {
                normalize_json_schema_type(schema_type);
            }
            for child in object.values_mut() {
                normalize_json_schema_types(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                normalize_json_schema_types(child);
            }
        }
        _ => {}
    }
}

fn normalize_json_schema_type(value: &mut Value) {
    match value {
        Value::String(schema_type) if is_json_schema_type(schema_type) => {
            schema_type.make_ascii_lowercase();
        }
        Value::Array(schema_types) => {
            for schema_type in schema_types {
                normalize_json_schema_type(schema_type);
            }
        }
        _ => {}
    }
}

fn is_json_schema_type(value: &str) -> bool {
    matches!(
        value,
        "NULL" | "BOOLEAN" | "OBJECT" | "ARRAY" | "NUMBER" | "INTEGER" | "STRING"
    )
}

fn convert_message(msg: &NeutralMessage) -> Result<Value, ProxyError> {
    let role_str = match msg.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };

    if msg.blocks.len() == 1 {
        if let NeutralContentBlock::Text(ref text) = msg.blocks[0] {
            return Ok(json!({
                "role": role_str,
                "content": text
            }));
        }
    }

    let mut contents = Vec::new();
    let mut reasoning_contents = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_call_id = None;

    for block in &msg.blocks {
        match block {
            NeutralContentBlock::Text(text) => {
                contents.push(json!({
                    "type": "text",
                    "text": text
                }));
            }
            NeutralContentBlock::InlineData {
                mime_type,
                data_base64,
            } => {
                if input_modality_for_mime_type(mime_type) == ModelModality::Image {
                    contents.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", mime_type, data_base64)
                        }
                    }));
                } else if let Some(format) = openai_input_audio_format(mime_type) {
                    contents.push(json!({
                        "type": "input_audio",
                        "input_audio": {
                            "data": data_base64,
                            "format": format
                        }
                    }));
                } else {
                    contents.push(json!({
                        "type": "file",
                        "file": {
                            "filename": inline_data_filename(mime_type),
                            "file_data": data_base64
                        }
                    }));
                }
            }
            NeutralContentBlock::ToolCall {
                id,
                name,
                arguments_json,
            } => {
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments_json
                    }
                }));
            }
            NeutralContentBlock::ToolResult {
                tool_call_id: id,
                content,
                ..
            } => {
                tool_call_id = Some(id.clone());
                contents.push(json!({
                    "type": "text",
                    "text": content
                }));
            }
            NeutralContentBlock::Thinking { text, .. } => {
                reasoning_contents.push(text.as_str());
            }
        }
    }

    let mut obj = json!({
        "role": role_str,
    });

    if msg.role == MessageRole::Tool {
        if let Some(id) = tool_call_id {
            obj["tool_call_id"] = json!(id);
        }
        if !contents.is_empty() {
            let first_text = contents[0]["text"].as_str().unwrap_or_default();
            obj["content"] = json!(first_text);
        }
    } else {
        if !contents.is_empty() {
            obj["content"] = Value::Array(contents);
        }
        if !reasoning_contents.is_empty() {
            obj["reasoning_content"] = json!(reasoning_contents.join("\n"));
        }
        if !tool_calls.is_empty() {
            obj["tool_calls"] = Value::Array(tool_calls);
        }
    }

    Ok(obj)
}

/// 从请求消息中提取图片生成的文本提示词（取最后一条用户消息的文本）。
fn image_generation_prompt(request: &NeutralChatRequest) -> String {
    for message in request.messages.iter().rev() {
        if message.role != MessageRole::User {
            continue;
        }
        let mut prompt = String::new();
        for block in &message.blocks {
            if let NeutralContentBlock::Text(text) = block {
                if !prompt.is_empty() {
                    prompt.push('\n');
                }
                prompt.push_str(text);
            }
        }
        if !prompt.is_empty() {
            return prompt;
        }
    }
    String::new()
}

/// 由 Gemini 形 `imageConfig`（`aspectRatio` 等）推导 OpenAI 的 `size`，
/// 优先透传用户显式提供的 OpenAI 原生字段。
fn image_generation_size(image_config: &Option<Value>) -> Option<String> {
    let config = image_config.as_ref()?;
    if let Some(size) = config.get("size").and_then(Value::as_str) {
        return Some(size.to_string());
    }
    let aspect = config
        .get("aspectRatio")
        .or_else(|| config.get("aspect_ratio"))
        .and_then(Value::as_str)?;
    match aspect {
        "1:1" => Some("1024x1024".to_string()),
        "16:9" => Some("1536x1024".to_string()),
        "9:16" => Some("1024x1536".to_string()),
        "4:3" => Some("1024x768".to_string()),
        "3:4" => Some("768x1024".to_string()),
        "21:9" => Some("1792x768".to_string()),
        "9:21" => Some("768x1792".to_string()),
        _ => None,
    }
}

/// 将图片生成请求转换为 OpenAI images 端点（`/v1/images/generations`）的请求体。
fn build_image_generation_payload(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<Value, ProxyError> {
    let prompt = image_generation_prompt(request);
    if prompt.trim().is_empty() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            "Image generation requires a non-empty text prompt",
            400,
        ));
    }

    let mut payload = json!({
        "model": route.upstream_model.upstream_model_id,
        "prompt": prompt,
        "n": 1,
        "response_format": "b64_json",
    });

    if let Some(size) = image_generation_size(&request.image_generation_config) {
        payload["size"] = json!(size);
    }
    if let Some(quality) = request
        .image_generation_config
        .as_ref()
        .and_then(|config| config.get("quality"))
        .cloned()
    {
        payload["quality"] = quality;
    }

    // extra_body 允许用户覆盖/补充 images 参数（如 size、quality、style）。
    if let Some(ref extra) = route.final_parameters.extra_body {
        for (key, value) in extra {
            payload[key] = value.clone();
        }
    }

    Ok(payload)
}

pub(super) fn build_request_payload(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<Value, ProxyError> {
    if is_image_generation_request(&route.upstream_model, request) {
        return build_image_generation_payload(route, request);
    }

    let mut payload = json!({
        "model": route.upstream_model.upstream_model_id,
        "stream": request.stream,
    });

    let mut messages_json = Vec::new();
    if let Some(ref sys) = request.system_instruction {
        messages_json.push(json!({
            "role": "system",
            "content": sys
        }));
    }

    for msg in &request.messages {
        messages_json.push(convert_message(msg)?);
    }
    payload["messages"] = Value::Array(messages_json);

    if !request.tools.is_empty() {
        let tools_json: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let mut parameters = t.function.parameters_schema.clone();
                normalize_json_schema_types(&mut parameters);
                json!({
                    "type": "function",
                    "function": {
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": parameters
                    }
                })
            })
            .collect();
        payload["tools"] = Value::Array(tools_json);
    }

    let params = &route.final_parameters;
    if let Some(temp) = params.temperature {
        payload["temperature"] = json!(temp);
    }
    if let Some(max_t) = params.max_tokens {
        payload["max_tokens"] = json!(max_t);
    }
    if let Some(top_p) = params.top_p {
        payload["top_p"] = json!(top_p);
    }

    if let Some(ref extra) = params.extra_body {
        for (k, v) in extra {
            payload[k] = v.clone();
        }
    }

    if let Some(level) = route.final_reasoning_level {
        let mapping = route
            .upstream_model
            .capabilities
            .reasoning
            .mapping_for(level)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!(
                        "No reasoning mapping configured for OpenAI level {:?}",
                        level
                    ),
                    400,
                )
            })?;
        match mapping {
            ReasoningMapping::Effort(effort) => {
                payload["reasoning_effort"] = json!(effort);
            }
            ReasoningMapping::Disabled => {
                payload["reasoning_effort"] = json!("none");
            }
            _ => {
                return Err(ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!("OpenAI does not support reasoning mapping: {:?}", mapping),
                    400,
                ));
            }
        }
    }

    if request.stream {
        if !payload["stream_options"].is_object() {
            payload["stream_options"] = json!({});
        }
        payload["stream_options"]["include_usage"] = json!(true);
    }

    Ok(payload)
}

pub(super) fn build_headers(provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    if !provider.api_key.is_empty() {
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", provider.api_key),
        );
    }

    for (k, v) in &provider.headers {
        headers.insert(k.clone(), v.clone());
    }

    Ok(headers)
}
