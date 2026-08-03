use crate::domain::model::ReasoningMapping;
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, NeutralMessage, Provider,
    ProxyError, UpstreamModel,
};
use crate::routing::ResolvedRoute;
use reqwest::Url;
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) fn build_generate_endpoint(
    provider: &Provider,
    upstream_model: &UpstreamModel,
    stream: bool,
) -> Result<String, ProxyError> {
    let expanded = provider
        .generate_endpoint
        .replace("{model}", &upstream_model.upstream_model_id);
    let mut url = Url::parse(&expanded).map_err(|error| {
        ProxyError::new(
            ErrorCategory::InvalidRequest,
            format!("Invalid Gemini generate endpoint: {error}"),
            400,
        )
    })?;

    let path = url.path().to_string();
    let resolved_path = if stream {
        path.strip_suffix(":generateContent")
            .map(|prefix| format!("{prefix}:streamGenerateContent"))
            .unwrap_or(path)
    } else {
        path.strip_suffix(":streamGenerateContent")
            .map(|prefix| format!("{prefix}:generateContent"))
            .unwrap_or(path)
    };
    url.set_path(&resolved_path);

    let mut query_pairs = url.query_pairs().into_owned().collect::<Vec<_>>();
    if stream {
        let mut has_alt = false;
        for (name, value) in &mut query_pairs {
            if name == "alt" {
                *value = "sse".to_string();
                has_alt = true;
            }
        }
        if !has_alt {
            query_pairs.push(("alt".to_string(), "sse".to_string()));
        }
    } else {
        query_pairs.retain(|(name, value)| !(name == "alt" && value == "sse"));
    }
    url.set_query(None);
    if !query_pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(
            query_pairs
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );
    }

    Ok(url.to_string())
}

pub(super) fn convert_message(msg: &NeutralMessage) -> Value {
    let role_str = match msg.role {
        MessageRole::User | MessageRole::System => "user",
        MessageRole::Assistant => "model",
        MessageRole::Tool => "user",
    };

    let mut parts = Vec::new();
    for block in &msg.blocks {
        match block {
            NeutralContentBlock::Text(text) => {
                parts.push(json!({ "text": text }));
            }
            NeutralContentBlock::Image {
                mime_type,
                data_base64,
            } => {
                parts.push(json!({
                    "inlineData": {
                        "mimeType": mime_type,
                        "data": data_base64
                    }
                }));
            }
            NeutralContentBlock::ToolCall {
                id,
                name,
                arguments_json,
            } => {
                let args: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
                parts.push(json!({
                    "functionCall": {
                        "id": id,
                        "name": name,
                        "args": args
                    }
                }));
            }
            NeutralContentBlock::ToolResult {
                tool_call_id,
                name,
                content,
            } => {
                let response_val: Value =
                    serde_json::from_str(content).unwrap_or_else(|_| json!({ "result": content }));
                parts.push(json!({
                    "functionResponse": {
                        "id": tool_call_id,
                        "name": name.as_deref().unwrap_or(tool_call_id),
                        "response": response_val
                    }
                }));
            }
            NeutralContentBlock::Thinking { text, signature } => {
                let mut part = json!({
                    "thought": true,
                    "text": text
                });
                if let Some(signature) = signature {
                    part["thoughtSignature"] = json!(signature);
                }
                parts.push(part);
            }
        }
    }

    json!({
        "role": role_str,
        "parts": parts
    })
}

fn write_reasoning(payload: &mut Value, route: &ResolvedRoute) -> Result<(), ProxyError> {
    let Some(level) = route.final_reasoning_level else {
        return Ok(());
    };

    let mapping = route
        .upstream_model
        .capabilities
        .reasoning
        .mapping_for(level)
        .ok_or_else(|| {
            ProxyError::new(
                ErrorCategory::UnsupportedFeature,
                format!(
                    "No reasoning mapping configured for Gemini level {:?}",
                    level
                ),
                400,
            )
        })?;

    let (field, value) = match mapping {
        ReasoningMapping::Disabled => ("thinkingBudget", json!(0)),
        ReasoningMapping::BudgetTokens(tokens) => ("thinkingBudget", json!(tokens)),
        ReasoningMapping::NativeLevel(level) => ("thinkingLevel", json!(level)),
        ReasoningMapping::Effort(_) | ReasoningMapping::Adaptive => {
            return Err(ProxyError::new(
                ErrorCategory::UnsupportedFeature,
                format!("Gemini does not support reasoning mapping {:?}", mapping),
                400,
            ));
        }
    };

    let generation_config = payload
        .as_object_mut()
        .expect("Gemini payload must be an object")
        .entry("generationConfig")
        .or_insert_with(|| json!({}));
    if !generation_config.is_object() {
        *generation_config = json!({});
    }

    let thinking_config = generation_config
        .as_object_mut()
        .expect("generationConfig must be an object")
        .entry("thinkingConfig")
        .or_insert_with(|| json!({}));
    if !thinking_config.is_object() {
        *thinking_config = json!({});
    }
    let thinking_config = thinking_config
        .as_object_mut()
        .expect("thinkingConfig must be an object");
    if field == "thinkingBudget" {
        thinking_config.remove("thinkingLevel");
    } else {
        thinking_config.remove("thinkingBudget");
    }
    thinking_config.insert(field.to_string(), value);

    Ok(())
}

pub(super) fn build_request_payload(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<Value, ProxyError> {
    let mut contents = Vec::new();
    for msg in &request.messages {
        contents.push(convert_message(msg));
    }

    let mut payload = json!({
        "contents": contents
    });

    if let Some(ref sys) = request.system_instruction {
        payload["systemInstruction"] = json!({
            "parts": [{ "text": sys }]
        });
    }

    if !request.tools.is_empty() {
        let func_decls: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "parameters": tool.function.parameters_schema
                })
            })
            .collect();
        payload["tools"] = json!([{
            "functionDeclarations": func_decls
        }]);
    }

    let params = &route.final_parameters;
    let mut gen_config = json!({});
    if let Some(temp) = params.temperature {
        gen_config["temperature"] = json!(temp);
    }
    if let Some(max_t) = params.max_tokens {
        gen_config["maxOutputTokens"] = json!(max_t);
    }
    if let Some(top_p) = params.top_p {
        gen_config["topP"] = json!(top_p);
    }
    if let Some(top_k) = params.top_k {
        gen_config["topK"] = json!(top_k);
    }

    if !gen_config.as_object().unwrap().is_empty() {
        payload["generationConfig"] = gen_config;
    }

    if let Some(ref extra) = params.extra_body {
        for (k, v) in extra {
            payload[k] = v.clone();
        }
    }

    write_reasoning(&mut payload, route)?;

    Ok(payload)
}

pub(super) fn build_headers(provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    if !provider.api_key.is_empty() {
        headers.insert("x-goog-api-key".to_string(), provider.api_key.clone());
    }

    for (k, v) in &provider.headers {
        headers.insert(k.clone(), v.clone());
    }

    Ok(headers)
}
