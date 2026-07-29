use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    Openai,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ParameterOverrides {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub extra_body: Option<HashMap<String, serde_json::Value>>,
}

impl ParameterOverrides {
    pub fn merge_with(&self, child: &ParameterOverrides) -> ParameterOverrides {
        let mut merged_extra = self.extra_body.clone().unwrap_or_default();
        if let Some(ref child_extra) = child.extra_body {
            for (k, v) in child_extra {
                merged_extra.insert(k.clone(), v.clone());
            }
        }

        ParameterOverrides {
            temperature: child.temperature.or(self.temperature),
            max_tokens: child.max_tokens.or(self.max_tokens),
            top_p: child.top_p.or(self.top_p),
            top_k: child.top_k.or(self.top_k),
            extra_body: if merged_extra.is_empty() {
                None
            } else {
                Some(merged_extra)
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub models_endpoint: String,
    pub generate_endpoint: String,
    #[serde(default)]
    pub api_key: String,
    pub headers: HashMap<String, String>,
    pub default_parameters: ParameterOverrides,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub stream_idle_timeout_ms: u64,
    pub enabled: bool,
}
