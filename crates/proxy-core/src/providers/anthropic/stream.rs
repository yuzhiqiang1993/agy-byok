use super::normalize_finish_reason;
use crate::domain::{ErrorCategory, NeutralStreamEvent, ProxyError, UpstreamModel, UsageInfo};
use crate::providers::traits::ProviderStreamDecoder;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicContentBlockKind {
    Text,
    Thinking,
    ToolUse,
    Other,
}

struct AnthropicStreamDecoder {
    fallback_model: String,
    message_started: bool,
    finish_emitted: bool,
    response_ended: bool,
    usage: UsageInfo,
    open_blocks: BTreeMap<u32, AnthropicContentBlockKind>,
    thinking_signatures: BTreeMap<u32, String>,
}

impl AnthropicStreamDecoder {
    fn new(upstream_model: &UpstreamModel) -> Self {
        Self {
            fallback_model: upstream_model.upstream_model_id.clone(),
            message_started: false,
            finish_emitted: false,
            response_ended: false,
            usage: UsageInfo::default(),
            open_blocks: BTreeMap::new(),
            thinking_signatures: BTreeMap::new(),
        }
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }

    fn ensure_active_message(&self, event_type: &str) -> Result<(), ProxyError> {
        if self.response_ended {
            return Err(Self::stream_error(format!(
                "Anthropic {event_type} received after response end"
            )));
        }
        if !self.message_started {
            return Err(Self::stream_error(format!(
                "Anthropic {event_type} received before message_start"
            )));
        }
        Ok(())
    }

    fn required_object<'a>(
        value: &'a Value,
        field: &str,
        event_type: &str,
    ) -> Result<&'a serde_json::Map<String, Value>, ProxyError> {
        value.get(field).and_then(Value::as_object).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} is missing object field {field}"
            ))
        })
    }

    fn required_index(value: &Value, event_type: &str) -> Result<u32, ProxyError> {
        let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} is missing integer field index"
            ))
        })?;
        u32::try_from(index).map_err(|_| {
            Self::stream_error(format!(
                "Anthropic {event_type} index exceeds the supported range"
            ))
        })
    }

    fn parse_usage(
        value: Option<&Value>,
        current: &UsageInfo,
        event_type: &str,
    ) -> Result<Option<UsageInfo>, ProxyError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        let usage = value.as_object().ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic {event_type} usage must be a JSON object"
            ))
        })?;

        let parse_token = |field: &str| -> Result<Option<u32>, ProxyError> {
            let Some(value) = usage.get(field) else {
                return Ok(None);
            };
            let token_count = value.as_u64().ok_or_else(|| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage.{field} must be a non-negative integer"
                ))
            })?;
            u32::try_from(token_count).map(Some).map_err(|_| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage.{field} exceeds the supported range"
                ))
            })
        };

        let prompt_tokens = parse_token("input_tokens")?.unwrap_or(current.prompt_tokens);
        let completion_tokens = parse_token("output_tokens")?.unwrap_or(current.completion_tokens);
        let total_tokens = prompt_tokens
            .checked_add(completion_tokens)
            .ok_or_else(|| {
                Self::stream_error(format!(
                    "Anthropic {event_type} usage token total exceeds the supported range"
                ))
            })?;

        Ok(Some(UsageInfo {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }))
    }

    fn require_block_kind(
        &self,
        index: u32,
        expected: AnthropicContentBlockKind,
        delta_type: &str,
    ) -> Result<(), ProxyError> {
        match self.open_blocks.get(&index) {
            Some(kind) if *kind == expected => Ok(()),
            Some(_) => Err(Self::stream_error(format!(
                "Anthropic {delta_type} does not match content block {index}"
            ))),
            None => Err(Self::stream_error(format!(
                "Anthropic {delta_type} references unopened content block {index}"
            ))),
        }
    }

    fn decode_message_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Err(Self::stream_error(
                "Anthropic message_start received after response end",
            ));
        }
        if self.message_started {
            return Err(Self::stream_error(
                "Anthropic stream contains multiple message_start events",
            ));
        }

        let message = Self::required_object(value, "message", "message_start")?;
        let response_id = match message.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.clone()),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_start message.id must be a string",
                ));
            }
        };
        let model = match message.get("model") {
            None | Some(Value::Null) => self.fallback_model.clone(),
            Some(Value::String(model)) => model.clone(),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_start message.model must be a string",
                ));
            }
        };
        let usage = Self::parse_usage(message.get("usage"), &self.usage, "message_start")?;

        if let Some(usage) = usage {
            self.usage = usage;
        }
        self.message_started = true;

        Ok(vec![NeutralStreamEvent::ResponseStart {
            response_id,
            model,
        }])
    }

    fn decode_content_block_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_start")?;
        let index = Self::required_index(value, "content_block_start")?;
        if self.open_blocks.contains_key(&index) {
            return Err(Self::stream_error(format!(
                "Anthropic content block {index} was started more than once"
            )));
        }

        let content_block = Self::required_object(value, "content_block", "content_block_start")?;
        let block_type = content_block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Self::stream_error(
                    "Anthropic content_block_start is missing string field content_block.type",
                )
            })?;

        match block_type {
            "tool_use" => {
                let id = content_block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic tool_use content block is missing string field id",
                        )
                    })?
                    .to_string();
                let name = content_block
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic tool_use content block is missing string field name",
                        )
                    })?
                    .to_string();
                let initial_arguments = match content_block.get("input") {
                    None | Some(Value::Null) => None,
                    Some(Value::Object(input)) if input.is_empty() => None,
                    Some(input @ Value::Object(_)) => {
                        Some(serde_json::to_string(input).map_err(|error| {
                            Self::stream_error(format!(
                                "Failed to encode Anthropic tool_use input: {error}"
                            ))
                        })?)
                    }
                    Some(_) => {
                        return Err(Self::stream_error(
                            "Anthropic tool_use content block input must be a JSON object",
                        ));
                    }
                };

                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::ToolUse);
                let mut events = vec![NeutralStreamEvent::ToolCallStart {
                    choice_index: 0,
                    tool_call_index: index,
                    id,
                    name,
                }];
                if let Some(arguments_delta) = initial_arguments {
                    events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                        choice_index: 0,
                        tool_call_index: index,
                        arguments_delta,
                    });
                }
                Ok(events)
            }
            "text" => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Text);
                Ok(Vec::new())
            }
            "thinking" => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Thinking);
                let signature = content_block
                    .get("signature")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.thinking_signatures.insert(index, signature);
                Ok(Vec::new())
            }
            _ => {
                self.open_blocks
                    .insert(index, AnthropicContentBlockKind::Other);
                Ok(Vec::new())
            }
        }
    }

    fn decode_content_block_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_delta")?;
        let delta = Self::required_object(value, "delta", "content_block_delta")?;
        let delta_type = delta.get("type").and_then(Value::as_str).ok_or_else(|| {
            Self::stream_error("Anthropic content_block_delta is missing string field delta.type")
        })?;

        match delta_type {
            "text_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::Text, delta_type)?;
                let text = delta.get("text").and_then(Value::as_str).ok_or_else(|| {
                    Self::stream_error("Anthropic text_delta is missing string field text")
                })?;
                Ok(vec![NeutralStreamEvent::TextDelta {
                    choice_index: 0,
                    text: text.to_string(),
                }])
            }
            "thinking_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::Thinking, delta_type)?;
                let text = delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic thinking_delta is missing string field thinking",
                        )
                    })?;
                Ok(vec![NeutralStreamEvent::ThinkingDelta {
                    choice_index: 0,
                    text: text.to_string(),
                }])
            }
            "signature_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::Thinking, delta_type)?;
                let signature =
                    delta
                        .get("signature")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            Self::stream_error(
                                "Anthropic signature_delta is missing string field signature",
                            )
                        })?;
                self.thinking_signatures
                    .entry(index)
                    .or_default()
                    .push_str(signature);
                Ok(Vec::new())
            }
            "input_json_delta" => {
                let index = Self::required_index(value, "content_block_delta")?;
                self.require_block_kind(index, AnthropicContentBlockKind::ToolUse, delta_type)?;
                let partial_json = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Self::stream_error(
                            "Anthropic input_json_delta is missing string field partial_json",
                        )
                    })?;
                Ok(vec![NeutralStreamEvent::ToolCallArgumentsDelta {
                    choice_index: 0,
                    tool_call_index: index,
                    arguments_delta: partial_json.to_string(),
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn decode_content_block_stop(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("content_block_stop")?;
        let index = Self::required_index(value, "content_block_stop")?;
        let block_kind = self.open_blocks.remove(&index).ok_or_else(|| {
            Self::stream_error(format!(
                "Anthropic content_block_stop references unopened content block {index}"
            ))
        })?;

        if block_kind == AnthropicContentBlockKind::ToolUse {
            Ok(vec![NeutralStreamEvent::ToolCallEnd {
                choice_index: 0,
                tool_call_index: index,
            }])
        } else if block_kind == AnthropicContentBlockKind::Thinking {
            let signature = self.thinking_signatures.remove(&index).unwrap_or_default();
            if signature.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![NeutralStreamEvent::ThinkingSignature {
                    choice_index: 0,
                    signature,
                }])
            }
        } else {
            Ok(Vec::new())
        }
    }

    fn decode_message_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        self.ensure_active_message("message_delta")?;
        let usage = Self::parse_usage(value.get("usage"), &self.usage, "message_delta")?;
        let delta = Self::required_object(value, "delta", "message_delta")?;
        let stop_reason = match delta.get("stop_reason") {
            None | Some(Value::Null) => None,
            Some(Value::String(reason)) => Some(reason.clone()),
            Some(_) => {
                return Err(Self::stream_error(
                    "Anthropic message_delta delta.stop_reason must be a string",
                ));
            }
        };

        let mut events = Vec::new();
        if let Some(usage) = usage {
            self.usage = usage.clone();
            events.push(NeutralStreamEvent::UsageUpdate(usage));
        }
        if let Some(raw_finish_reason) = stop_reason {
            if !self.finish_emitted {
                self.finish_emitted = true;
                events.push(NeutralStreamEvent::Finish {
                    choice_index: 0,
                    reason: normalize_finish_reason(&raw_finish_reason),
                    raw_finish_reason: Some(raw_finish_reason),
                });
            }
        }
        Ok(events)
    }

    fn close_response(&mut self) -> Vec<NeutralStreamEvent> {
        if self.response_ended {
            return Vec::new();
        }

        let mut events = self
            .open_blocks
            .iter()
            .filter_map(|(index, block_kind)| {
                (*block_kind == AnthropicContentBlockKind::ToolUse).then_some(
                    NeutralStreamEvent::ToolCallEnd {
                        choice_index: 0,
                        tool_call_index: *index,
                    },
                )
            })
            .collect::<Vec<_>>();
        events.extend(
            std::mem::take(&mut self.thinking_signatures)
                .into_values()
                .filter(|signature| !signature.is_empty())
                .map(|signature| NeutralStreamEvent::ThinkingSignature {
                    choice_index: 0,
                    signature,
                }),
        );
        self.open_blocks.clear();
        self.response_ended = true;
        events.push(NeutralStreamEvent::ResponseEnd);
        events
    }

    fn decode_message_stop(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        self.ensure_active_message("message_stop")?;
        Ok(self.close_response())
    }
}

impl ProviderStreamDecoder for AnthropicStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        let value: Value = serde_json::from_str(data).map_err(|error| {
            Self::stream_error(format!(
                "Failed to parse Anthropic stream event JSON: {error}"
            ))
        })?;
        let event_type = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            Self::stream_error("Anthropic stream event is missing string field type")
        })?;

        match event_type {
            "message_start" => self.decode_message_start(&value),
            "content_block_start" => self.decode_content_block_start(&value),
            "content_block_delta" => self.decode_content_block_delta(&value),
            "content_block_stop" => self.decode_content_block_stop(&value),
            "message_delta" => self.decode_message_delta(&value),
            "message_stop" => self.decode_message_stop(),
            _ => Ok(Vec::new()),
        }
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(self.close_response())
    }
}

pub(super) fn create_stream_decoder(
    upstream_model: &UpstreamModel,
) -> Box<dyn ProviderStreamDecoder> {
    Box::new(AnthropicStreamDecoder::new(upstream_model))
}
