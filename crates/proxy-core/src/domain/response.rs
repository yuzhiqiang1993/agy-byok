use super::request::NeutralContentBlock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UsageInfo {
    /// Non-cached input tokens.
    pub input_tokens: u32,
    /// Visible output tokens, excluding separately reported reasoning tokens.
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub total_tokens: u32,
}

impl UsageInfo {
    pub fn from_aggregate_totals(
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: Option<u32>,
        cache_read_tokens: Option<u32>,
        cache_write_tokens: Option<u32>,
        reasoning_tokens: Option<u32>,
    ) -> Self {
        let (input_tokens, cache_read_tokens, cache_write_tokens) = match cache_read_tokens
            .unwrap_or(0)
            .checked_add(cache_write_tokens.unwrap_or(0))
            .filter(|details| *details <= prompt_tokens)
        {
            Some(details) => (
                prompt_tokens - details,
                cache_read_tokens,
                cache_write_tokens,
            ),
            None => (prompt_tokens, None, None),
        };
        let (output_tokens, reasoning_tokens) = match reasoning_tokens {
            Some(reasoning) if reasoning <= completion_tokens => {
                (completion_tokens - reasoning, Some(reasoning))
            }
            Some(_) => (completion_tokens, None),
            None => (completion_tokens, None),
        };
        let computed_total = prompt_tokens.saturating_add(completion_tokens);

        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            total_tokens: total_tokens
                .filter(|reported| *reported >= computed_total)
                .unwrap_or(computed_total),
        }
    }

    pub fn prompt_tokens(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.cache_read_tokens.unwrap_or(0))
            .saturating_add(self.cache_write_tokens.unwrap_or(0))
    }

    pub fn completion_tokens(&self) -> u32 {
        self.output_tokens
            .saturating_add(self.reasoning_tokens.unwrap_or(0))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    MaxTokens,
    ToolCall,
    ContentFilter,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChoice {
    pub index: u32,
    pub blocks: Vec<NeutralContentBlock>,
    pub finish_reason: Option<FinishReason>,
    pub raw_finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeutralChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<NeutralChoice>,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NeutralStreamEvent {
    ResponseStart {
        response_id: Option<String>,
        model: String,
    },
    TextDelta {
        choice_index: u32,
        text: String,
    },
    ThinkingDelta {
        choice_index: u32,
        text: String,
    },
    ThinkingSignature {
        choice_index: u32,
        signature: String,
    },
    ToolCallStart {
        choice_index: u32,
        tool_call_index: u32,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        choice_index: u32,
        tool_call_index: u32,
        arguments_delta: String,
    },
    ToolCallEnd {
        choice_index: u32,
        tool_call_index: u32,
    },
    Finish {
        choice_index: u32,
        reason: FinishReason,
        raw_finish_reason: Option<String>,
    },
    ResponseEnd {
        usage: Option<UsageInfo>,
    },
    Error {
        message: String,
        code: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_usage_is_split_into_disjoint_dimensions() {
        let usage = UsageInfo::from_aggregate_totals(12, 9, Some(21), Some(5), None, Some(4));

        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, Some(5));
        assert_eq!(usage.reasoning_tokens, Some(4));
        assert_eq!(usage.prompt_tokens(), 12);
        assert_eq!(usage.completion_tokens(), 9);
        assert_eq!(usage.total_tokens, 21);
    }

    #[test]
    fn invalid_detail_breakdowns_do_not_inflate_aggregate_totals() {
        let usage = UsageInfo::from_aggregate_totals(3, 2, Some(1), Some(4), Some(1), Some(3));

        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.cache_write_tokens, None);
        assert_eq!(usage.reasoning_tokens, None);
        assert_eq!(usage.total_tokens, 5);
    }
}
