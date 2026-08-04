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
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
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
