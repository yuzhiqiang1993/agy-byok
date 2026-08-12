use super::{normalize_finish_reason, parse_usage};
use crate::domain::{ErrorCategory, NeutralStreamEvent, ProxyError, UpstreamModel, UsageInfo};
use crate::providers::traits::ProviderStreamDecoder;
use serde_json::Value;
use std::collections::HashSet;

pub(super) struct GeminiStreamDecoder {
    model: String,
    response_started: bool,
    response_ended: bool,
    emitted_tool_calls: HashSet<(u32, u32)>,
    emitted_thinking_signatures: HashSet<(u32, u32)>,
    finished_choices: HashSet<u32>,
    usage: Option<UsageInfo>,
}

impl GeminiStreamDecoder {
    pub(super) fn new(model: String) -> Self {
        Self {
            model,
            response_started: false,
            response_ended: false,
            emitted_tool_calls: HashSet::new(),
            emitted_thinking_signatures: HashSet::new(),
            finished_choices: HashSet::new(),
            usage: None,
        }
    }

    fn response_end(&mut self) -> Vec<NeutralStreamEvent> {
        if self.response_ended {
            Vec::new()
        } else {
            self.response_ended = true;
            vec![NeutralStreamEvent::ResponseEnd {
                usage: self.usage.clone(),
            }]
        }
    }
}

impl ProviderStreamDecoder for GeminiStreamDecoder {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        if self.response_ended {
            return Ok(Vec::new());
        }
        if data.trim() == "[DONE]" {
            return Ok(self.response_end());
        }

        let value: Value = serde_json::from_str(data).map_err(|error| {
            ProxyError::new(
                ErrorCategory::StreamInterrupted,
                format!("Failed to parse Gemini stream data: {error}"),
                502,
            )
        })?;

        let mut events = Vec::new();
        if !self.response_started {
            self.response_started = true;
            events.push(NeutralStreamEvent::ResponseStart {
                response_id: value
                    .get("responseId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                model: self.model.clone(),
            });
        }

        let mut finish_events = Vec::new();
        if let Some(candidates) = value.get("candidates").and_then(Value::as_array) {
            for (candidate_position, candidate) in candidates.iter().enumerate() {
                let choice_index = candidate
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or(candidate_position as u32);

                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
                {
                    for (part_position, part) in parts.iter().enumerate() {
                        let part_index = part_position as u32;
                        if part
                            .get("thought")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                events.push(NeutralStreamEvent::ThinkingDelta {
                                    choice_index,
                                    text: text.to_string(),
                                });
                            }
                            if let Some(signature) = part
                                .get("thoughtSignature")
                                .and_then(Value::as_str)
                                .filter(|signature| !signature.is_empty())
                            {
                                if self
                                    .emitted_thinking_signatures
                                    .insert((choice_index, part_index))
                                {
                                    events.push(NeutralStreamEvent::ThinkingSignature {
                                        choice_index,
                                        signature: signature.to_string(),
                                    });
                                }
                            }
                        } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                            events.push(NeutralStreamEvent::TextDelta {
                                choice_index,
                                text: text.to_string(),
                            });
                        } else if let Some(inline_data) =
                            part.get("inlineData").or_else(|| part.get("inline_data"))
                        {
                            let data_base64 = inline_data
                                .get("data")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if !data_base64.is_empty() {
                                let mime_type = inline_data
                                    .get("mimeType")
                                    .or_else(|| inline_data.get("mime_type"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("image/png");
                                events.push(NeutralStreamEvent::InlineData {
                                    choice_index,
                                    mime_type: mime_type.to_string(),
                                    data_base64: data_base64.to_string(),
                                });
                            }
                        } else if let Some(function_call) = part.get("functionCall") {
                            if self.emitted_tool_calls.insert((choice_index, part_index)) {
                                let name = function_call
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                let arguments = function_call
                                    .get("args")
                                    .cloned()
                                    .unwrap_or(Value::Null)
                                    .to_string();
                                let id = function_call
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .filter(|id| !id.is_empty())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| format!("call_{choice_index}_{part_index}"));
                                events.push(NeutralStreamEvent::ToolCallStart {
                                    choice_index,
                                    tool_call_index: part_index,
                                    id,
                                    name,
                                });
                                events.push(NeutralStreamEvent::ToolCallArgumentsDelta {
                                    choice_index,
                                    tool_call_index: part_index,
                                    arguments_delta: arguments,
                                });
                                events.push(NeutralStreamEvent::ToolCallEnd {
                                    choice_index,
                                    tool_call_index: part_index,
                                });
                            }
                        }
                    }
                }

                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    if self.finished_choices.insert(choice_index) {
                        finish_events.push(NeutralStreamEvent::Finish {
                            choice_index,
                            reason: normalize_finish_reason(reason),
                            raw_finish_reason: Some(reason.to_string()),
                        });
                    }
                }
            }
        }

        let has_candidates = value
            .get("candidates")
            .and_then(Value::as_array)
            .is_some_and(|candidates| !candidates.is_empty());
        if !has_candidates {
            if let Some(reason) = value
                .get("promptFeedback")
                .and_then(|feedback| feedback.get("blockReason"))
                .and_then(Value::as_str)
            {
                if self.finished_choices.insert(0) {
                    finish_events.push(NeutralStreamEvent::Finish {
                        choice_index: 0,
                        reason: normalize_finish_reason(reason),
                        raw_finish_reason: Some(reason.to_string()),
                    });
                }
            }
        }

        if let Some(usage) = value
            .get("usageMetadata")
            .and_then(|usage| parse_usage(usage, self.usage.as_ref()))
        {
            self.usage = Some(usage);
        }

        events.extend(finish_events);
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(self.response_end())
    }
}

pub(super) fn create_stream_decoder(
    upstream_model: &UpstreamModel,
) -> Box<dyn ProviderStreamDecoder> {
    Box::new(GeminiStreamDecoder::new(
        upstream_model.upstream_model_id.clone(),
    ))
}
