use super::traits::{ProviderAdapter, ProviderStreamDecoder};
use crate::domain::model::ReasoningMapping;
use crate::domain::response::{FinishReason, NeutralChoice};
use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralChatResponse, NeutralContentBlock,
    NeutralMessage, NeutralStreamEvent, Provider, ProxyError, UpstreamModel, UsageInfo,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct OpenAIResponsesAdapter;

impl OpenAIResponsesAdapter {
    pub fn new() -> Self {
        Self
    }

    fn normalize_finish_reason(raw: &str) -> FinishReason {
        match raw {
            "completed" | "stop" => FinishReason::Stop,
            "max_output_tokens" | "length" | "incomplete" => FinishReason::MaxTokens,
            "tool_calls" | "function_call" => FinishReason::ToolCall,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        }
    }

    fn parse_usage(value: &Value) -> Option<UsageInfo> {
        value.as_object().map(|usage| UsageInfo {
            prompt_tokens: usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            completion_tokens: usage
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            total_tokens: usage
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        })
    }

    fn normalize_json_schema_types(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if let Some(schema_type) = object.get_mut("type") {
                    Self::normalize_json_schema_type(schema_type);
                }
                for child in object.values_mut() {
                    Self::normalize_json_schema_types(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    Self::normalize_json_schema_types(child);
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
                    Self::normalize_json_schema_type(schema_type);
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

    fn convert_message(message: &NeutralMessage) -> Vec<Value> {
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
                    "type": Self::input_content_type(&message.role),
                    "text": text,
                })),
                NeutralContentBlock::Image {
                    mime_type,
                    data_base64,
                } => content.push(json!({
                    "type": "input_image",
                    "image_url": format!("data:{};base64,{}", mime_type, data_base64),
                })),
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
        items
    }

    fn parse_output_blocks(output: &Value) -> Vec<NeutralContentBlock> {
        let mut blocks = Vec::new();
        let Some(items) = output.as_array() else {
            return blocks;
        };

        for item in items {
            match item["type"].as_str().unwrap_or_default() {
                "message" => {
                    if let Some(content) = item["content"].as_array() {
                        for part in content {
                            match part["type"].as_str().unwrap_or_default() {
                                "output_text" => {
                                    if let Some(text) = part["text"].as_str() {
                                        if !text.is_empty() {
                                            blocks
                                                .push(NeutralContentBlock::Text(text.to_string()));
                                        }
                                    }
                                }
                                "refusal" => {
                                    if let Some(refusal) = part["refusal"].as_str() {
                                        if !refusal.is_empty() {
                                            blocks.push(NeutralContentBlock::Text(
                                                refusal.to_string(),
                                            ));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "reasoning" => {
                    if let Some(summary) = item["summary"].as_array() {
                        for part in summary {
                            if let Some(text) = part["text"].as_str() {
                                blocks.push(NeutralContentBlock::Thinking {
                                    text: text.to_string(),
                                    signature: None,
                                });
                            }
                        }
                    }
                }
                "function_call" => {
                    let id = item["call_id"]
                        .as_str()
                        .or_else(|| item["id"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    let name = item["name"].as_str().unwrap_or_default().to_string();
                    let arguments_json = item["arguments"].as_str().unwrap_or("{}").to_string();
                    blocks.push(NeutralContentBlock::ToolCall {
                        id,
                        name,
                        arguments_json,
                    });
                }
                _ => {}
            }
        }
        blocks
    }

    fn parse_error(status: u16, body: &str) -> ProxyError {
        let category = match status {
            401 | 403 => ErrorCategory::Authentication,
            404 => ErrorCategory::ModelNotFound,
            429 => ErrorCategory::RateLimit,
            500..=599 => ErrorCategory::UpstreamServerError,
            _ => ErrorCategory::InvalidRequest,
        };
        ProxyError::new(
            category,
            format!("OpenAI Responses upstream status {status}"),
            status,
        )
        .with_upstream_body(body)
    }
}

#[derive(Default)]
struct ResponsesToolState {
    id: Option<String>,
    name: Option<String>,
    pending_arguments: Vec<String>,
    started: bool,
    ended: bool,
    saw_argument_delta: bool,
}

struct OpenAIResponsesStreamDecoder {
    fallback_model: String,
    response_started: bool,
    response_ended: bool,
    tools: HashMap<u32, ResponsesToolState>,
    tool_order: Vec<u32>,
    finished: bool,
}

impl OpenAIResponsesStreamDecoder {
    fn new(fallback_model: String) -> Self {
        Self {
            fallback_model,
            response_started: false,
            response_ended: false,
            tools: HashMap::new(),
            tool_order: Vec::new(),
            finished: false,
        }
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }

    fn output_index(value: &Value) -> u32 {
        value["output_index"]
            .as_u64()
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0)
    }

    fn start_response_if_needed(&mut self, value: &Value, events: &mut Vec<NeutralStreamEvent>) {
        if self.response_started {
            return;
        }
        let response = value.get("response").unwrap_or(value);
        events.push(NeutralStreamEvent::ResponseStart {
            response_id: response["id"].as_str().map(str::to_string),
            model: response["model"]
                .as_str()
                .unwrap_or(&self.fallback_model)
                .to_string(),
        });
        self.response_started = true;
    }

    fn tool_state_mut(&mut self, index: u32) -> &mut ResponsesToolState {
        let tool_order = &mut self.tool_order;
        self.tools.entry(index).or_insert_with(|| {
            tool_order.push(index);
            ResponsesToolState::default()
        })
    }

    fn update_tool_metadata(&mut self, index: u32, item: &Value) {
        let state = self.tool_state_mut(index);
        if state.id.is_none() {
            state.id = item["call_id"]
                .as_str()
                .or_else(|| item["id"].as_str())
                .map(str::to_string);
        }
        if state.name.is_none() {
            state.name = item["name"].as_str().map(str::to_string);
        }
    }

    fn maybe_start_tool(
        &mut self,
        index: u32,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let (id, name, pending_arguments) = {
            let state = self.tool_state_mut(index);
            if state.started {
                return Ok(());
            }
            let (Some(id), Some(name)) = (state.id.clone(), state.name.clone()) else {
                return Ok(());
            };
            state.started = true;
            (id, name, std::mem::take(&mut state.pending_arguments))
        };
        events.push(NeutralStreamEvent::ToolCallStart {
            choice_index: 0,
            tool_call_index: index,
            id,
            name,
        });
        for arguments_delta in pending_arguments {
            events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index: 0,
                tool_call_index: index,
                arguments_delta,
            });
        }
        Ok(())
    }

    fn decode_output_item_added(
        &mut self,
        value: &Value,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let item = &value["item"];
        if item["type"].as_str() != Some("function_call") {
            return Ok(());
        }
        let index = Self::output_index(value);
        self.update_tool_metadata(index, item);
        self.maybe_start_tool(index, events)
    }

    fn decode_function_arguments_delta(
        &mut self,
        value: &Value,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let index = Self::output_index(value);
        let delta = value["delta"].as_str().unwrap_or_default().to_string();
        if delta.is_empty() {
            return Ok(());
        }
        let started = {
            let state = self.tool_state_mut(index);
            state.saw_argument_delta = true;
            if state.started {
                true
            } else {
                state.pending_arguments.push(delta.clone());
                false
            }
        };
        self.maybe_start_tool(index, events)?;
        if started {
            events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                choice_index: 0,
                tool_call_index: index,
                arguments_delta: delta,
            });
        }
        Ok(())
    }

    fn decode_output_item_done(
        &mut self,
        value: &Value,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let item = &value["item"];
        if item["type"].as_str() != Some("function_call") {
            return Ok(());
        }
        let index = Self::output_index(value);
        self.update_tool_metadata(index, item);
        self.maybe_start_tool(index, events)?;
        let (started, saw_delta, final_arguments) = {
            let state = self.tool_state_mut(index);
            (
                state.started,
                state.saw_argument_delta,
                item["arguments"].as_str().map(str::to_string),
            )
        };
        if !started {
            return Err(Self::stream_error(format!(
                "OpenAI Responses function call {index} ended without call_id and name"
            )));
        }
        if !saw_delta {
            if let Some(arguments) = final_arguments.filter(|arguments| !arguments.is_empty()) {
                events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: index,
                    arguments_delta: arguments,
                });
            }
        }
        self.end_tool(index, events)
    }

    fn end_tool(
        &mut self,
        index: u32,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let state = self
            .tools
            .get_mut(&index)
            .ok_or_else(|| Self::stream_error(format!("Unknown Responses tool call {index}")))?;
        if !state.ended {
            state.ended = true;
            events.push(NeutralStreamEvent::ToolCallEnd {
                choice_index: 0,
                tool_call_index: index,
            });
        }
        Ok(())
    }

    fn close_tools(&mut self, events: &mut Vec<NeutralStreamEvent>) -> Result<(), ProxyError> {
        for index in self.tool_order.clone() {
            if let Some(state) = self.tools.get(&index) {
                if state.started && !state.ended {
                    self.end_tool(index, events)?;
                }
            }
        }
        Ok(())
    }

    fn finish_response(
        &mut self,
        response: &Value,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        if self.response_ended {
            return Ok(());
        }
        if let Some(usage) = OpenAIResponsesAdapter::parse_usage(&response["usage"]) {
            events.push(NeutralStreamEvent::UsageUpdate(usage));
        }
        self.close_tools(events)?;
        if !self.finished {
            let raw_reason = response["incomplete_details"]["reason"]
                .as_str()
                .or_else(|| response["status"].as_str())
                .unwrap_or("completed")
                .to_string();
            let reason = if self
                .tool_order
                .iter()
                .any(|index| self.tools.get(index).is_some_and(|state| state.started))
            {
                FinishReason::ToolCall
            } else {
                OpenAIResponsesAdapter::normalize_finish_reason(&raw_reason)
            };
            events.push(NeutralStreamEvent::Finish {
                choice_index: 0,
                reason,
                raw_finish_reason: Some(raw_reason),
            });
            self.finished = true;
        }
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd);
        Ok(())
    }
}

impl ProviderStreamDecoder for OpenAIResponsesStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(data).map_err(|error| {
            Self::stream_error(format!(
                "Failed to parse OpenAI Responses streaming JSON response: {error}"
            ))
        })?;
        let mut events = Vec::new();
        self.start_response_if_needed(&value, &mut events);

        match value["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                if let Some(delta) = value["delta"].as_str().filter(|delta| !delta.is_empty()) {
                    events.push(NeutralStreamEvent::TextDelta {
                        choice_index: 0,
                        text: delta.to_string(),
                    });
                }
            }
            "response.reasoning_summary_text.delta" => {
                if let Some(delta) = value["delta"].as_str().filter(|delta| !delta.is_empty()) {
                    events.push(NeutralStreamEvent::ThinkingDelta {
                        choice_index: 0,
                        text: delta.to_string(),
                    });
                }
            }
            "response.output_item.added" => {
                self.decode_output_item_added(&value, &mut events)?;
            }
            "response.function_call_arguments.delta" => {
                self.decode_function_arguments_delta(&value, &mut events)?;
            }
            "response.output_item.done" => {
                self.decode_output_item_done(&value, &mut events)?;
            }
            "response.completed" | "response.incomplete" => {
                let response = value.get("response").unwrap_or(&value);
                self.finish_response(response, &mut events)?;
            }
            "response.failed" => {
                return Err(Self::stream_error(
                    value["response"]["error"]["message"]
                        .as_str()
                        .unwrap_or("OpenAI Responses response failed"),
                ));
            }
            _ => {}
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        self.close_tools(&mut events)?;
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd);
        Ok(events)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAIResponsesAdapter {
    fn build_request_payload(
        &self,
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
            input.extend(Self::convert_message(message));
        }
        payload["input"] = Value::Array(input);

        if !request.tools.is_empty() {
            let tools = request
                .tools
                .iter()
                .map(|tool| {
                    let mut parameters = tool.function.parameters_schema.clone();
                    Self::normalize_json_schema_types(&mut parameters);
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
                ReasoningMapping::Effort(effort) => {
                    payload["reasoning"] = json!({ "effort": effort });
                }
                ReasoningMapping::Disabled => {}
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

    fn build_headers(&self, provider: &Provider) -> Result<HashMap<String, String>, ProxyError> {
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

    fn parse_response(
        &self,
        status: u16,
        body: &str,
        upstream_model: &UpstreamModel,
    ) -> Result<NeutralChatResponse, ProxyError> {
        if status >= 400 {
            return Err(Self::parse_error(status, body));
        }
        let value: Value = serde_json::from_str(body).map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to parse OpenAI Responses JSON response: {error}"),
                500,
            )
        })?;
        let blocks = Self::parse_output_blocks(&value["output"]);
        let status = value["status"].as_str().unwrap_or("completed");
        let raw_finish_reason = value["incomplete_details"]["reason"]
            .as_str()
            .or(Some(status))
            .map(str::to_string);
        let finish_reason = if blocks
            .iter()
            .any(|block| matches!(block, NeutralContentBlock::ToolCall { .. }))
        {
            Some(FinishReason::ToolCall)
        } else {
            raw_finish_reason
                .as_deref()
                .map(OpenAIResponsesAdapter::normalize_finish_reason)
        };

        Ok(NeutralChatResponse {
            id: value["id"].as_str().unwrap_or("resp-id").to_string(),
            model: value["model"]
                .as_str()
                .unwrap_or(&upstream_model.upstream_model_id)
                .to_string(),
            choices: vec![NeutralChoice {
                index: 0,
                blocks,
                finish_reason,
                raw_finish_reason,
            }],
            usage: Self::parse_usage(&value["usage"]),
        })
    }

    fn create_stream_decoder(
        &self,
        upstream_model: &UpstreamModel,
    ) -> Box<dyn ProviderStreamDecoder> {
        Box::new(OpenAIResponsesStreamDecoder::new(
            upstream_model.upstream_model_id.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_message_tool_call_reasoning_and_usage() {
        let adapter = OpenAIResponsesAdapter::new();
        let model = UpstreamModel {
            id: "upstream".to_string(),
            provider_id: "provider".to_string(),
            upstream_model_id: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            capabilities: Default::default(),
            parameter_overrides: Default::default(),
            enabled: true,
        };
        let response = adapter
            .parse_response(
                200,
                r#"{
                    "id":"resp_1",
                    "model":"gpt-5",
                    "status":"completed",
                    "output":[
                      {"type":"reasoning","summary":[{"type":"summary_text","text":"Plan"}]},
                      {"type":"message","content":[{"type":"output_text","text":"Done"}]},
                      {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"q\":\"x\"}"}
                    ],
                    "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}
                }"#,
                &model,
            )
            .unwrap();
        assert_eq!(response.id, "resp_1");
        assert_eq!(response.usage.unwrap().total_tokens, 5);
        assert!(matches!(
            response.choices[0].finish_reason,
            Some(FinishReason::ToolCall)
        ));
        assert!(response.choices[0]
            .blocks
            .iter()
            .any(|block| matches!(block, NeutralContentBlock::Thinking { .. })));
        assert!(response.choices[0].blocks.iter().any(
            |block| matches!(block, NeutralContentBlock::ToolCall { id, .. } if id == "call_1")
        ));
    }

    #[test]
    fn decodes_responses_text_and_completed_events() {
        let mut decoder = OpenAIResponsesStreamDecoder::new("gpt-5".to_string());
        let start = decoder
            .decode_data(
                r#"{"type":"response.created","response":{"id":"resp_1","model":"gpt-5"}}"#,
            )
            .unwrap();
        assert!(matches!(start[0], NeutralStreamEvent::ResponseStart { .. }));
        let delta = decoder
            .decode_data(r#"{"type":"response.output_text.delta","delta":"Hello"}"#)
            .unwrap();
        assert!(matches!(&delta[0], NeutralStreamEvent::TextDelta { text, .. } if text == "Hello"));
        let end = decoder
            .decode_data(r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}}"#)
            .unwrap();
        assert!(end.iter().any(|event| matches!(
            event,
            NeutralStreamEvent::Finish {
                reason: FinishReason::Stop,
                ..
            }
        )));
        assert!(end
            .iter()
            .any(|event| matches!(event, NeutralStreamEvent::ResponseEnd)));
    }
}
