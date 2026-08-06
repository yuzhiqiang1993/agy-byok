use super::{normalize_finish_reason, parse_index, parse_usage};
use crate::domain::{ErrorCategory, NeutralStreamEvent, ProxyError, UpstreamModel, UsageInfo};
use crate::providers::traits::ProviderStreamDecoder;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

type ToolCallKey = (u32, u32);

#[derive(Default)]
struct OpenAIToolCallState {
    id: Option<String>,
    name: Option<String>,
    pending_arguments: Vec<String>,
    started: bool,
    ended: bool,
}

struct OpenAIStreamDecoder {
    fallback_model: String,
    response_started: bool,
    response_ended: bool,
    tools: HashMap<ToolCallKey, OpenAIToolCallState>,
    tool_order: Vec<ToolCallKey>,
    finished_choices: HashSet<u32>,
    usage: Option<UsageInfo>,
}

impl OpenAIStreamDecoder {
    fn new(fallback_model: String) -> Self {
        Self {
            fallback_model,
            response_started: false,
            response_ended: false,
            tools: HashMap::new(),
            tool_order: Vec::new(),
            finished_choices: HashSet::new(),
            usage: None,
        }
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }

    fn decode_tool_delta(
        &mut self,
        choice_index: u32,
        tool_call_index: u32,
        tool_call: &Value,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let key = (choice_index, tool_call_index);
        if !self.tools.contains_key(&key) {
            self.tool_order.push(key);
            self.tools.insert(key, OpenAIToolCallState::default());
        }

        let state = self
            .tools
            .get_mut(&key)
            .expect("OpenAI tool state must exist after insertion");
        if state.ended {
            return Err(Self::stream_error(format!(
                "OpenAI tool call choice {} index {} received data after it ended",
                choice_index, tool_call_index
            )));
        }

        if !state.started {
            if let Some(id) = tool_call["id"].as_str().filter(|id| !id.is_empty()) {
                state.id = Some(id.to_string());
            }
            if let Some(name) = tool_call["function"]["name"]
                .as_str()
                .filter(|name| !name.is_empty())
            {
                state.name = Some(name.to_string());
            }
        }

        let arguments_delta = tool_call["function"]["arguments"]
            .as_str()
            .map(str::to_string);
        if state.started {
            if let Some(arguments_delta) = arguments_delta {
                events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index,
                    tool_call_index,
                    arguments_delta,
                });
            }
            return Ok(());
        }

        if let Some(arguments_delta) = arguments_delta {
            state.pending_arguments.push(arguments_delta);
        }

        if let (Some(id), Some(name)) = (&state.id, &state.name) {
            events.push(NeutralStreamEvent::ToolCallStart {
                choice_index,
                tool_call_index,
                id: id.clone(),
                name: name.clone(),
            });
            state.started = true;
            for arguments_delta in state.pending_arguments.drain(..) {
                events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index,
                    tool_call_index,
                    arguments_delta,
                });
            }
        }

        Ok(())
    }

    fn close_tool_keys(
        &mut self,
        keys: &[ToolCallKey],
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        for &(choice_index, tool_call_index) in keys {
            let state = self
                .tools
                .get(&(choice_index, tool_call_index))
                .expect("OpenAI tool state must exist for recorded key");
            if !state.ended && !state.started {
                return Err(Self::stream_error(format!(
                    "OpenAI tool call choice {} index {} ended without both id and name",
                    choice_index, tool_call_index
                )));
            }
        }

        for &(choice_index, tool_call_index) in keys {
            let state = self
                .tools
                .get_mut(&(choice_index, tool_call_index))
                .expect("OpenAI tool state must exist for recorded key");
            if !state.ended {
                state.ended = true;
                events.push(NeutralStreamEvent::ToolCallEnd {
                    choice_index,
                    tool_call_index,
                });
            }
        }
        Ok(())
    }

    fn close_tools_for_choice(
        &mut self,
        choice_index: u32,
        events: &mut Vec<NeutralStreamEvent>,
    ) -> Result<(), ProxyError> {
        let keys = self
            .tool_order
            .iter()
            .copied()
            .filter(|(tool_choice_index, _)| *tool_choice_index == choice_index)
            .collect::<Vec<_>>();
        self.close_tool_keys(&keys, events)
    }

    fn end_response(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }

        let keys = self.tool_order.clone();
        let mut events = Vec::new();
        self.close_tool_keys(&keys, &mut events)?;
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd {
            usage: self.usage.clone(),
        });
        Ok(events)
    }
}

impl ProviderStreamDecoder for OpenAIStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        if data.trim() == "[DONE]" {
            return self.end_response();
        }

        let value: Value = serde_json::from_str(data).map_err(|error| {
            Self::stream_error(format!(
                "Failed to parse OpenAI streaming JSON response: {}",
                error
            ))
        })?;
        let mut events = Vec::new();

        if !self.response_started {
            events.push(NeutralStreamEvent::ResponseStart {
                response_id: value["id"].as_str().map(str::to_string),
                model: value["model"]
                    .as_str()
                    .unwrap_or(&self.fallback_model)
                    .to_string(),
            });
            self.response_started = true;
        }

        let mut finishes = Vec::new();
        if let Some(choices) = value["choices"].as_array() {
            for (choice_position, choice) in choices.iter().enumerate() {
                let choice_index = parse_index(choice, choice_position);
                let delta = &choice["delta"];

                if let Some(content) = delta["content"].as_str() {
                    if !content.is_empty() {
                        events.push(NeutralStreamEvent::TextDelta {
                            choice_index,
                            text: content.to_string(),
                        });
                    }
                }

                if let Some(reasoning) = delta["reasoning_content"].as_str() {
                    if !reasoning.is_empty() {
                        events.push(NeutralStreamEvent::ThinkingDelta {
                            choice_index,
                            text: reasoning.to_string(),
                        });
                    }
                }

                if let Some(tool_calls) = delta["tool_calls"].as_array() {
                    for (tool_call_position, tool_call) in tool_calls.iter().enumerate() {
                        let tool_call_index = parse_index(tool_call, tool_call_position);
                        self.decode_tool_delta(
                            choice_index,
                            tool_call_index,
                            tool_call,
                            &mut events,
                        )?;
                    }
                }

                if let Some(raw_finish_reason) = choice["finish_reason"].as_str() {
                    finishes.push((choice_index, raw_finish_reason.to_string()));
                }
            }
        }

        if let Some(usage) = parse_usage(&value["usage"]) {
            self.usage = Some(usage);
        }

        for (choice_index, raw_finish_reason) in finishes {
            self.close_tools_for_choice(choice_index, &mut events)?;
            if self.finished_choices.insert(choice_index) {
                events.push(NeutralStreamEvent::Finish {
                    choice_index,
                    reason: normalize_finish_reason(&raw_finish_reason),
                    raw_finish_reason: Some(raw_finish_reason),
                });
            }
        }

        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.end_response()
    }
}

pub(super) fn create_stream_decoder(
    upstream_model: &UpstreamModel,
) -> Box<dyn ProviderStreamDecoder> {
    Box::new(OpenAIStreamDecoder::new(
        upstream_model.upstream_model_id.clone(),
    ))
}
