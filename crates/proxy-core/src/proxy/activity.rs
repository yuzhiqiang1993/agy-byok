use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_ACTIVITY_ITEMS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    pub operation: String,
    pub request_method: String,
    pub request_path: String,
    pub request_body_bytes: Option<u64>,
    pub response_body_bytes: Option<u64>,
    pub response_summary: Option<String>,
    pub timestamp_ms: u64,
    pub requested_virtual_model_id: String,
    pub virtual_model_id: String,
    pub upstream_model_id: Option<String>,
    pub provider_id: String,
    pub provider_protocol: Option<String>,
    pub status_code: u16,
    pub duration_ms: u64,
    pub error_category: Option<String>,
    pub error_detail: Option<String>,
    pub stream: bool,
    pub message_count: usize,
    pub tool_count: usize,
    pub used_fallback: bool,
    pub fallback_attempted: bool,
    pub fallback_succeeded: bool,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

pub struct ActivityLog {
    items: Mutex<VecDeque<ActivityItem>>,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityLog {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(MAX_ACTIVITY_ITEMS)),
        }
    }

    pub fn record(&self, item: ActivityItem) {
        let mut guard = self.items.lock().unwrap();
        if guard.len() >= MAX_ACTIVITY_ITEMS {
            guard.pop_front();
        }
        guard.push_back(item);
    }

    pub fn get_recent(&self) -> Vec<ActivityItem> {
        let guard = self.items.lock().unwrap();
        guard.iter().cloned().collect()
    }

    pub fn clear(&self) {
        let mut guard = self.items.lock().unwrap();
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_serializes_all_usage_dimensions_for_the_frontend() {
        let value = serde_json::to_value(ActivityItem {
            id: "activity-1".to_string(),
            kind: "chat".to_string(),
            operation: "generate".to_string(),
            request_method: "POST".to_string(),
            request_path: "/generate".to_string(),
            request_body_bytes: None,
            response_body_bytes: None,
            response_summary: None,
            timestamp_ms: 1,
            requested_virtual_model_id: "virtual".to_string(),
            virtual_model_id: "virtual".to_string(),
            upstream_model_id: Some("upstream".to_string()),
            provider_id: "provider".to_string(),
            provider_protocol: Some("openai".to_string()),
            status_code: 200,
            duration_ms: 10,
            error_category: None,
            error_detail: None,
            stream: true,
            message_count: 1,
            tool_count: 0,
            used_fallback: false,
            fallback_attempted: false,
            fallback_succeeded: false,
            input_tokens: Some(7),
            output_tokens: Some(5),
            cache_read_tokens: Some(3),
            cache_write_tokens: Some(2),
            reasoning_tokens: Some(4),
            total_tokens: Some(21),
        })
        .unwrap();

        assert_eq!(value["inputTokens"], 7);
        assert_eq!(value["outputTokens"], 5);
        assert_eq!(value["cacheReadTokens"], 3);
        assert_eq!(value["cacheWriteTokens"], 2);
        assert_eq!(value["reasoningTokens"], 4);
        assert_eq!(value["totalTokens"], 21);
    }
}
