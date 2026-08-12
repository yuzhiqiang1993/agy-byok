use crate::domain::model::ReasoningMapping;
use crate::domain::{
    is_supported_inline_document_mime_type, is_supported_inline_image_mime_type, ErrorCategory,
    MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage, Provider, ProxyError,
};
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
        Value::String(schema_type)
            if matches!(
                schema_type.as_str(),
                "NULL" | "BOOLEAN" | "OBJECT" | "ARRAY" | "NUMBER" | "INTEGER" | "STRING"
            ) =>
        {
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

fn input_content_type(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::Assistant => "output_text",
        _ => "input_text",
    }
}

fn convert_message(message: &NeutralMessage) -> Result<Vec<Value>, ProxyError> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let mut content = Vec::new();
    let mut function_calls = Vec::new();
    let mut function_outputs = Vec::new();

    for block in &message.blocks {
        match block {
            NeutralContentBlock::Text(text) => content.push(json!({
                "type": input_content_type(&message.role),
                "text": text,
            })),
            NeutralContentBlock::InlineData {
                mime_type,
                data_base64,
            } => {
                if is_supported_inline_image_mime_type(mime_type) {
                    content.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{};base64,{}", mime_type, data_base64),
                    }));
                } else if is_supported_inline_document_mime_type(mime_type) {
                    content.push(json!({
                        "type": "input_file",
                        "filename": "input.pdf",
                        "file_data": data_base64,
                    }));
                } else {
                    return Err(ProxyError::new(
                        ErrorCategory::UnsupportedFeature,
                        format!("OpenAI Responses does not support inline {mime_type} content"),
                        400,
                    ));
                }
            }
            NeutralContentBlock::ToolCall {
                id,
                name,
                arguments_json,
            } => function_calls.push(json!({
                "type": "function_call",
                "call_id": id,
                "name": name,
                "arguments": arguments_json,
            })),
            NeutralContentBlock::ToolResult {
                tool_call_id: id,
                content: output,
                ..
            } => function_outputs.push(json!({
                "type": "function_call_output",
                "call_id": id,
                "output": output,
            })),
            // Responses accepts reasoning summaries, not the provider's private
            // reasoning tokens. Do not replay private thinking as user-visible input.
            NeutralContentBlock::Thinking { .. } => {}
        }
    }

    let mut items = Vec::new();
    if !content.is_empty() {
        items.push(json!({
            "role": role,
            "content": content,
        }));
    }
    items.extend(function_calls);
    items.extend(function_outputs);
    Ok(items)
}

pub(super) fn build_request_payload(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<Value, ProxyError> {
    let mut payload = json!({
        "model": route.upstream_model.upstream_model_id,
        "input": [],
        "stream": request.stream,
    });
    if let Some(system_instruction) = &request.system_instruction {
        payload["instructions"] = json!(system_instruction);
    }

    let mut input = Vec::new();
    for message in &request.messages {
        input.extend(convert_message(message)?);
    }
    payload["input"] = Value::Array(input);

    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                let mut parameters = tool.function.parameters_schema.clone();
                normalize_json_schema_types(&mut parameters);
                json!({
                    "type": "function",
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "parameters": parameters,
                })
            })
            .collect::<Vec<_>>();
        payload["tools"] = Value::Array(tools);
    }

    let params = &route.final_parameters;
    if let Some(temperature) = params.temperature {
        payload["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = params.max_tokens {
        payload["max_output_tokens"] = json!(max_tokens);
    }
    if let Some(top_p) = params.top_p {
        payload["top_p"] = json!(top_p);
    }
    if let Some(extra) = &params.extra_body {
        for (key, value) in extra {
            payload[key] = value.clone();
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
                        "No reasoning mapping configured for OpenAI Responses level {:?}",
                        level
                    ),
                    400,
                )
            })?;
        match mapping {
            ReasoningMapping::Effort(effort) => match payload.get_mut("reasoning") {
                Some(Value::Object(reasoning)) => {
                    reasoning.insert("effort".to_string(), json!(effort));
                }
                _ => payload["reasoning"] = json!({ "effort": effort }),
            },
            ReasoningMapping::Disabled => {
                // 关闭档位必须清理 extra_body 中可能残留的 reasoning 配置。
                payload
                    .as_object_mut()
                    .expect("Responses payload must be an object")
                    .remove("reasoning");
            }
            _ => {
                return Err(ProxyError::new(
                    ErrorCategory::UnsupportedFeature,
                    format!(
                        "OpenAI Responses does not support reasoning mapping: {:?}",
                        mapping
                    ),
                    400,
                ));
            }
        }
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
    for (key, value) in &provider.headers {
        headers.insert(key.clone(), value.clone());
    }
    Ok(headers)
}
