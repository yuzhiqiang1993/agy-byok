use super::{normalize_finish_reason, parse_usage};
use crate::domain::response::FinishReason;
use crate::domain::{ErrorCategory, NeutralStreamEvent, ProxyError, UpstreamModel};
use crate::providers::traits::ProviderStreamDecoder;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
struct ResponsesToolState {
    id: Option<String>,
    name: Option<String>,
    pending_arguments: Vec<String>,
    started: bool,
    ended: bool,
    saw_argument_delta: bool,
}

pub(super) struct OpenAIResponsesStreamDecoder {
    fallback_model: String,
    response_started: bool,
    response_ended: bool,
    tools: HashMap<u32, ResponsesToolState>,
    tool_order: Vec<u32>,
    finished: bool,
}

impl OpenAIResponsesStreamDecoder {
    pub(super) fn new(fallback_model: String) -> Self {
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
        let usage = parse_usage(&response["usage"]);
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
                normalize_finish_reason(&raw_reason)
            };
            events.push(NeutralStreamEvent::Finish {
                choice_index: 0,
                reason,
                raw_finish_reason: Some(raw_reason),
            });
            self.finished = true;
        }
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd { usage });
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
        events.push(NeutralStreamEvent::ResponseEnd { usage: None });
        Ok(events)
    }
}

pub(super) fn create_stream_decoder(
    upstream_model: &UpstreamModel,
) -> Box<dyn ProviderStreamDecoder> {
    Box::new(OpenAIResponsesStreamDecoder::new(
        upstream_model.upstream_model_id.clone(),
    ))
}
