use super::serde_helpers::required_nullable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    OpenaiChatCompletions,
    AnthropicMessages,
    GeminiGenerateContent,
    OpenaiResponses,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ParameterOverrides {
    #[serde(deserialize_with = "required_nullable")]
    pub temperature: Option<f32>,
    #[serde(deserialize_with = "required_nullable")]
    pub max_tokens: Option<u32>,
    #[serde(deserialize_with = "required_nullable")]
    pub top_p: Option<f32>,
    #[serde(deserialize_with = "required_nullable")]
    pub top_k: Option<u32>,
    #[serde(deserialize_with = "required_nullable")]
    pub extra_body: Option<HashMap<String, serde_json::Value>>,
}

fn merge_json_value(parent: &mut serde_json::Value, child: &serde_json::Value) {
    match (parent, child) {
        (serde_json::Value::Object(parent_map), serde_json::Value::Object(child_map)) => {
            for (k, v) in child_map {
                match parent_map.get_mut(k) {
                    Some(parent_val) => merge_json_value(parent_val, v),
                    None => {
                        parent_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (parent_val, child_val) => {
            *parent_val = child_val.clone();
        }
    }
}

impl ParameterOverrides {
    pub fn merge_with(&self, child: &ParameterOverrides) -> ParameterOverrides {
        let mut merged_extra = self.extra_body.clone().unwrap_or_default();
        if let Some(ref child_extra) = child.extra_body {
            for (k, v) in child_extra {
                match merged_extra.get_mut(k) {
                    Some(existing_val) => merge_json_value(existing_val, v),
                    None => {
                        merged_extra.insert(k.clone(), v.clone());
                    }
                }
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
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub models_endpoint: String,
    pub generate_endpoint: String,
    pub api_key: String,
    pub headers: HashMap<String, String>,
    pub default_parameters: ParameterOverrides,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub stream_idle_timeout_ms: u64,
    pub enabled: bool,
}

#[cfg(test)]
mod extra_body_merge_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extra_body_deep_merges_nested_json_objects() {
        let mut parent_extra = HashMap::new();
        parent_extra.insert(
            "options".to_string(),
            json!({
                "nested_a": 1,
                "nested_obj": { "key_x": "val_x" }
            }),
        );
        let parent = ParameterOverrides {
            extra_body: Some(parent_extra),
            ..Default::default()
        };

        let mut child_extra = HashMap::new();
        child_extra.insert(
            "options".to_string(),
            json!({
                "nested_b": 2,
                "nested_obj": { "key_y": "val_y" }
            }),
        );
        let child = ParameterOverrides {
            extra_body: Some(child_extra),
            ..Default::default()
        };

        let merged = parent.merge_with(&child);
        let extra = merged.extra_body.unwrap();
        let options = extra.get("options").unwrap();

        assert_eq!(options["nested_a"], 1);
        assert_eq!(options["nested_b"], 2);
        assert_eq!(options["nested_obj"]["key_x"], "val_x");
        assert_eq!(options["nested_obj"]["key_y"], "val_y");
    }
}
