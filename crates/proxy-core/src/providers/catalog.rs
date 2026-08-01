use crate::domain::{ErrorCategory, Provider, ProviderProtocol, ProxyError, ReasoningLevel};
use crate::providers::get_adapter;
use crate::storage::AppConfig;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const CATALOG_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<ReasoningLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogModel {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ProviderCatalogReasoning>,
}

/// 使用供应商草稿直接拉取模型目录，允许用户在保存配置前验证连接。
pub async fn fetch_provider_models(
    provider: &Provider,
) -> Result<Vec<ProviderCatalogModel>, ProxyError> {
    AppConfig {
        providers: vec![provider.clone()],
        ..AppConfig::default()
    }
    .validate()
    .map_err(|message| ProxyError::new(ErrorCategory::InvalidRequest, message, 400))?;

    if provider.models_endpoint.trim().is_empty() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            "模型列表地址不能为空",
            400,
        ));
    }

    let timeout_ms = match provider.request_timeout_ms {
        0 => CATALOG_TIMEOUT_MS,
        configured => configured.min(CATALOG_TIMEOUT_MS),
    };
    let connect_timeout_ms = match provider.connect_timeout_ms {
        0 => 5_000,
        configured => configured.min(timeout_ms),
    };
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout_ms))
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("创建模型目录客户端失败：{error}"),
                500,
            )
        })?;
    let adapter = get_adapter(&provider.protocol);
    let mut request = client.get(&provider.models_endpoint);
    for (name, value) in adapter.build_headers(provider)? {
        request = request.header(name, value);
    }

    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            ProxyError::new(ErrorCategory::Timeout, "模型目录请求超时", 504)
        } else {
            ProxyError::new(
                ErrorCategory::ConnectionFailed,
                format!("无法连接模型列表地址：{error}"),
                502,
            )
        }
    })?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(ProxyError::new(
            catalog_error_category(status),
            format!("模型目录返回 HTTP {status}"),
            status,
        ));
    }
    let body = read_limited_response_body(response, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES)
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("读取模型目录响应失败：{error}"),
                500,
            )
        })?;
    if body.is_truncated() {
        return Err(ProxyError::new(
            ErrorCategory::UpstreamServerError,
            format!(
                "模型目录响应超过 {} 字节",
                DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES
            ),
            502,
        ));
    }
    let body = body.into_text();
    let payload: Value = serde_json::from_str(&body).map_err(|error| {
        ProxyError::new(
            ErrorCategory::Internal,
            format!("模型目录不是有效 JSON：{error}"),
            500,
        )
    })?;
    let mut models = parse_catalog_models(&payload, &provider.protocol);
    if models.is_empty() {
        return Err(ProxyError::new(
            ErrorCategory::Internal,
            "响应中没有可识别的模型列表",
            500,
        ));
    }
    models.sort_by_cached_key(|model| model.display_name.to_lowercase());
    Ok(models)
}

fn catalog_error_category(status: u16) -> ErrorCategory {
    match status {
        401 | 403 => ErrorCategory::Authentication,
        404 => ErrorCategory::ModelNotFound,
        429 => ErrorCategory::RateLimit,
        500..=599 => ErrorCategory::UpstreamServerError,
        _ => ErrorCategory::InvalidRequest,
    }
}

fn parse_catalog_models(payload: &Value, protocol: &ProviderProtocol) -> Vec<ProviderCatalogModel> {
    let items = payload
        .as_array()
        .or_else(|| payload.get("data").and_then(Value::as_array))
        .or_else(|| payload.get("models").and_then(Value::as_array));
    let mut seen = HashSet::new();

    items
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let raw_id = item.as_str().or_else(|| {
                item.get("id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("name").and_then(Value::as_str))
            })?;
            let id = normalize_model_id(raw_id, protocol);
            if id.is_empty() || !seen.insert(id.clone()) {
                return None;
            }
            let display_name = item
                .get("display_name")
                .and_then(Value::as_str)
                .or_else(|| item.get("displayName").and_then(Value::as_str))
                .unwrap_or(&id)
                .to_string();
            Some(ProviderCatalogModel {
                id,
                display_name,
                reasoning: parse_reasoning_metadata(item),
            })
        })
        .collect()
}

fn parse_reasoning_metadata(item: &Value) -> Option<ProviderCatalogReasoning> {
    let object = item.as_object()?;
    let mut supported = None;
    let mut levels = Vec::new();

    for key in [
        "reasoning",
        "thinking",
        "reasoning_capability",
        "reasoningCapability",
        "supports_reasoning",
        "supportsReasoning",
        "reasoning_levels",
        "reasoningLevels",
        "supported_reasoning_levels",
        "supportedReasoningLevels",
    ] {
        if let Some(value) = object.get(key) {
            collect_reasoning_metadata(value, &mut supported, &mut levels);
        }
    }

    for key in ["type", "model_type", "modelType"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let value = value.to_ascii_lowercase();
            if value.contains("reasoning") || value.contains("thinking") {
                supported = Some(true);
            }
        }
    }

    if let Some(capabilities) = object.get("capabilities") {
        if let Some(capabilities) = capabilities.as_object() {
            for key in [
                "reasoning",
                "thinking",
                "reasoning_levels",
                "reasoningLevels",
            ] {
                if let Some(value) = capabilities.get(key) {
                    collect_reasoning_metadata(value, &mut supported, &mut levels);
                }
            }
        } else if let Some(capabilities) = capabilities.as_array() {
            if capabilities.iter().filter_map(Value::as_str).any(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("reasoning") || value.contains("thinking")
            }) {
                supported = Some(true);
            }
        }
    }

    if let Some(parameters) = object.get("supported_parameters").and_then(Value::as_array) {
        if parameters
            .iter()
            .filter_map(Value::as_str)
            .any(|parameter| {
                let parameter = parameter.to_ascii_lowercase();
                parameter.contains("reasoning") || parameter.contains("thinking")
            })
        {
            supported = Some(true);
        }
    }

    levels.sort();
    levels.dedup();
    if !levels.is_empty() {
        supported = Some(true);
    }
    if supported.is_none() && levels.is_empty() {
        return None;
    }
    Some(ProviderCatalogReasoning { supported, levels })
}

fn collect_reasoning_metadata(
    value: &Value,
    supported: &mut Option<bool>,
    levels: &mut Vec<ReasoningLevel>,
) {
    match value {
        Value::Bool(value) => *supported = Some(*value),
        Value::String(value) => match value.to_ascii_lowercase().as_str() {
            "enabled" | "supported" | "adaptive" => *supported = Some(true),
            "disabled" | "unsupported" | "none" => *supported = Some(false),
            _ => add_reasoning_level(value, levels),
        },
        Value::Array(values) => {
            for value in values {
                if let Some(value) = value.as_str() {
                    add_reasoning_level(value, levels);
                } else {
                    collect_reasoning_metadata(value, supported, levels);
                }
            }
        }
        Value::Object(object) => {
            for key in [
                "levels",
                "supported_levels",
                "supportedLevels",
                "reasoning_levels",
                "reasoningLevels",
            ] {
                if let Some(value) = object.get(key) {
                    collect_reasoning_metadata(value, supported, levels);
                }
            }
            for key in [
                "supported",
                "enabled",
                "supports_reasoning",
                "supportsReasoning",
            ] {
                if let Some(value) = object.get(key).and_then(Value::as_bool) {
                    *supported = Some(value);
                }
            }
            if let Some(value) = object.get("type").and_then(Value::as_str) {
                match value.to_ascii_lowercase().as_str() {
                    "enabled" | "adaptive" => *supported = Some(true),
                    "disabled" | "none" => *supported = Some(false),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn add_reasoning_level(value: &str, levels: &mut Vec<ReasoningLevel>) {
    let normalized = value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-' && *character != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let level = match normalized.as_str() {
        "off" | "none" => ReasoningLevel::Off,
        "low" => ReasoningLevel::Low,
        "medium" | "med" => ReasoningLevel::Medium,
        "high" => ReasoningLevel::High,
        "xhigh" | "extrahigh" => ReasoningLevel::XHigh,
        "max" | "maximum" => ReasoningLevel::Max,
        "auto" | "adaptive" => ReasoningLevel::Auto,
        _ => return,
    };
    if !levels.contains(&level) {
        levels.push(level);
    }
}

fn normalize_model_id(value: &str, protocol: &ProviderProtocol) -> String {
    let value = value.trim();
    if matches!(protocol, ProviderProtocol::GeminiGenerateContent) {
        value.strip_prefix("models/").unwrap_or(value).to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ParameterOverrides;
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::HashMap;

    fn catalog_provider(models_endpoint: String) -> Provider {
        Provider {
            id: "provider-catalog".to_string(),
            name: "Catalog Provider".to_string(),
            protocol: ProviderProtocol::OpenaiChatCompletions,
            models_endpoint,
            generate_endpoint: "http://127.0.0.1:50998/v1/chat/completions".to_string(),
            api_key: "sk-catalog".to_string(),
            headers: HashMap::new(),
            default_parameters: ParameterOverrides::default(),
            connect_timeout_ms: 3000,
            request_timeout_ms: 5000,
            stream_idle_timeout_ms: 5000,
            enabled: true,
        }
    }

    #[test]
    fn parses_common_openai_and_gemini_catalog_shapes() {
        let openai = parse_catalog_models(
            &json!({
                "data": [
                    {"id": "gpt-5"},
                    {"id": "gpt-5"},
                    {"id": "gpt-4.1", "display_name": "GPT 4.1"}
                ]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(
            openai,
            vec![
                ProviderCatalogModel {
                    id: "gpt-5".to_string(),
                    display_name: "gpt-5".to_string(),
                    reasoning: None,
                },
                ProviderCatalogModel {
                    id: "gpt-4.1".to_string(),
                    display_name: "GPT 4.1".to_string(),
                    reasoning: None,
                },
            ]
        );

        let gemini = parse_catalog_models(
            &json!({
                "models": [
                    {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro"}
                ]
            }),
            &ProviderProtocol::GeminiGenerateContent,
        );
        assert_eq!(
            gemini,
            vec![ProviderCatalogModel {
                id: "gemini-2.5-pro".to_string(),
                display_name: "Gemini 2.5 Pro".to_string(),
                reasoning: None,
            }]
        );
    }

    #[test]
    fn parses_reasoning_metadata_without_assuming_missing_capability() {
        let models = parse_catalog_models(
            &json!({
                "data": [
                    {
                        "id": "claude-opus",
                        "thinking": {"supported": true, "levels": ["low", "high", "xhigh"]}
                    },
                    {"id": "plain-model"},
                    {"id": "no-thinking", "capabilities": {"reasoning": false}},
                    {"id": "router-model", "supported_parameters": ["reasoning_effort"]},
                    {"id": "modelgate-reasoning", "type": "Reasoning"}
                ]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );

        assert_eq!(
            models[0].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(true),
                levels: vec![
                    ReasoningLevel::Low,
                    ReasoningLevel::High,
                    ReasoningLevel::XHigh
                ],
            })
        );
        assert_eq!(models[1].reasoning, None);
        assert_eq!(
            models[2].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(false),
                levels: Vec::new(),
            })
        );
        assert_eq!(
            models[3].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(true),
                levels: Vec::new(),
            })
        );
        assert_eq!(
            models[4].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(true),
                levels: Vec::new(),
            })
        );
    }

    #[tokio::test]
    async fn fetches_catalog_with_provider_authentication() {
        let response = json!({
            "data": [
                {"id": "gpt-5.6-terra"},
                {"id": "gpt-5.6-sol"}
            ]
        })
        .to_string();
        let (mock_url, _handle, recorded) =
            MockProviderServer::start_recording(200, &response).await;

        let models = fetch_provider_models(&catalog_provider(format!("{mock_url}/v1/models")))
            .await
            .unwrap();

        assert_eq!(models.len(), 2);
        assert_eq!(
            recorded.await.unwrap().authorization.as_deref(),
            Some("Bearer sk-catalog")
        );
    }
}
