use crate::domain::{
    ErrorCategory, Provider, ProviderProtocol, ProxyError, ReasoningLevel, ReasoningMapping,
};
use crate::providers::get_adapter;
use crate::storage::AppConfig;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

const CATALOG_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<ReasoningLevel>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mappings: BTreeMap<ReasoningLevel, ReasoningMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogModel {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_token_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_token_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
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
    let endpoint = catalog_models_url(provider)?;
    let is_cpa_catalog = is_cpa_catalog_endpoint(&endpoint);
    let mut request = client.get(endpoint);
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
    let mut models =
        parse_catalog_models_with_context(&payload, &provider.protocol, is_cpa_catalog);
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

fn catalog_models_url(provider: &Provider) -> Result<Url, ProxyError> {
    let mut endpoint = Url::parse(&provider.models_endpoint).map_err(|error| {
        ProxyError::new(
            ErrorCategory::InvalidRequest,
            format!("模型目录地址无效：{error}"),
            400,
        )
    })?;

    if is_cpa_catalog_endpoint(&endpoint)
        && !endpoint
            .query_pairs()
            .any(|(key, _)| key == "client_version")
    {
        endpoint
            .query_pairs_mut()
            .append_pair("client_version", "1");
    }

    Ok(endpoint)
}

fn is_cpa_catalog_endpoint(endpoint: &Url) -> bool {
    let host_is_loopback = endpoint.host_str().is_some_and(|host| {
        let normalized = host.trim_start_matches('[').trim_end_matches(']');
        normalized == "localhost"
            || normalized
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    matches!(endpoint.port_or_known_default(), Some(8317)) && host_is_loopback
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

#[cfg(test)]
fn parse_catalog_models(payload: &Value, protocol: &ProviderProtocol) -> Vec<ProviderCatalogModel> {
    parse_catalog_models_with_context(payload, protocol, false)
}

fn parse_catalog_models_with_context(
    payload: &Value,
    protocol: &ProviderProtocol,
    is_cpa_catalog: bool,
) -> Vec<ProviderCatalogModel> {
    let items = catalog_items(payload);
    let mut seen = HashSet::new();

    items
        .into_iter()
        .filter_map(|(item, object_key)| {
            let raw_id = item.as_str().or(object_key).or_else(|| {
                item.get("id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("name").and_then(Value::as_str))
                    .or_else(|| item.get("slug").and_then(Value::as_str))
                    .or_else(|| item.get("model").and_then(Value::as_str))
                    .or_else(|| item.get("model_id").and_then(Value::as_str))
                    .or_else(|| item.get("modelId").and_then(Value::as_str))
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
            let exact_context_window =
                parse_token_limit(item, &["contextWindow", "context_window"]);
            let max_context_window =
                parse_token_limit(item, &["maxContextWindow", "max_context_window"]);
            let context_length = parse_token_limit(
                item,
                &[
                    "contextLength",
                    "context_length",
                    "maxContextLength",
                    "max_context_length",
                ],
            );
            // 某些目录只返回 max_context_window；context_window 对前端保留一个
            // 可直接展示的有效值，同时单独保留原生硬上限，避免丢失层级信息。
            let context_window = exact_context_window.or(max_context_window);
            let auto_compact_token_limit =
                parse_token_limit(item, &["autoCompactTokenLimit", "auto_compact_token_limit"]);
            let max_tokens = parse_token_limit(item, &["maxTokens", "max_tokens"]);
            let token_budget = parse_token_limit(item, &["tokenBudget", "token_budget"]);
            let explicit_input_token_limit = parse_token_limit(
                item,
                &[
                    "inputTokenLimit",
                    "input_token_limit",
                    "maxInputTokens",
                    "max_input_tokens",
                    "maxPromptTokens",
                    "max_prompt_tokens",
                ],
            );
            let input_token_limit = explicit_input_token_limit.or_else(|| {
                if is_cpa_catalog {
                    context_window.or(context_length)
                } else {
                    None
                }
            });
            let output_token_limit = parse_token_limit(
                item,
                &[
                    "outputTokenLimit",
                    "output_token_limit",
                    "maxOutputTokens",
                    "max_output_tokens",
                    "maxCompletionTokens",
                    "max_completion_tokens",
                ],
            )
            .or_else(|| {
                if matches!(protocol, ProviderProtocol::AnthropicMessages) || is_cpa_catalog {
                    max_tokens
                } else {
                    None
                }
            });
            Some(ProviderCatalogModel {
                id,
                display_name,
                context_window,
                max_context_window,
                context_length,
                auto_compact_token_limit,
                input_token_limit,
                output_token_limit,
                max_tokens,
                token_budget,
                capabilities: extract_capabilities(item),
                thinking: item.get("thinking").cloned(),
                reasoning: parse_reasoning_metadata(item, protocol),
            })
        })
        .collect()
}

/// 兼容 OpenAI/Gemini 的数组目录，也兼容 CPA 常见的模型 ID -> 元数据对象目录。
fn catalog_items<'a>(payload: &'a Value) -> Vec<(&'a Value, Option<&'a str>)> {
    if let Some(items) = payload.as_array() {
        return items.iter().map(|item| (item, None)).collect();
    }
    for container in ["data", "models"] {
        let Some(value) = payload.get(container) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            return items.iter().map(|item| (item, None)).collect();
        }
        if let Some(items) = value.as_object() {
            return items
                .iter()
                .map(|(key, item)| (item, Some(key.as_str())))
                .collect();
        }
    }
    payload
        .as_object()
        .filter(|object| object.values().any(Value::is_object))
        .map(|object| {
            object
                .iter()
                .map(|(key, item)| (item, Some(key.as_str())))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_capabilities(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    if let Some(capabilities) = object.get("capabilities") {
        return Some(capabilities.clone());
    }

    let mut capabilities = Map::new();
    for key in [
        "supportedGenerationMethods",
        "supported_generation_methods",
        "supportedParameters",
        "supported_parameters",
        "supportsImageInput",
        "supports_image_input",
        "supportsTools",
        "supports_tools",
        "vision",
        "tools",
    ] {
        if let Some(value) = object.get(key) {
            capabilities.insert(key.to_string(), value.clone());
        }
    }
    (!capabilities.is_empty()).then_some(Value::Object(capabilities))
}

fn parse_token_limit(item: &Value, keys: &[&str]) -> Option<u32> {
    fn find(value: &Value, keys: &[&str]) -> Option<u32> {
        let object = value.as_object()?;
        if let Some(limit) = keys
            .iter()
            .filter_map(|key| object.get(*key))
            .find_map(parse_positive_u32)
        {
            return Some(limit);
        }

        for container in [
            "limits",
            "metadata",
            "capabilities",
            "top_provider",
            "topProvider",
        ] {
            if let Some(nested) = object.get(container) {
                if let Some(limit) = find(nested, keys) {
                    return Some(limit);
                }
            }
        }
        None
    }

    find(item, keys)
}

fn parse_positive_u32(value: &Value) -> Option<u32> {
    let value = value.as_u64().or_else(|| {
        value
            .as_str()
            .and_then(|value| value.trim().parse::<u64>().ok())
    })?;
    (value > 0 && value <= u64::from(u32::MAX)).then_some(value as u32)
}

fn parse_reasoning_metadata(
    item: &Value,
    protocol: &ProviderProtocol,
) -> Option<ProviderCatalogReasoning> {
    let object = item.as_object()?;
    let mut supported = None;
    let mut levels = Vec::new();
    let mut mappings = BTreeMap::new();
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
        "thinking_levels",
        "thinkingLevels",
        "supported_thinking_levels",
        "supportedThinkingLevels",
        "reasoning_effort",
        "reasoningEffort",
        "effort",
        "supported_efforts",
        "supportedEfforts",
    ] {
        if let Some(value) = object.get(key) {
            collect_reasoning_metadata(value, protocol, &mut supported, &mut levels, &mut mappings);
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
                "effort",
                "supported_efforts",
                "supportedEfforts",
            ] {
                if let Some(value) = capabilities.get(key) {
                    collect_reasoning_metadata(
                        value,
                        protocol,
                        &mut supported,
                        &mut levels,
                        &mut mappings,
                    );
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
    if !levels.is_empty() || !mappings.is_empty() {
        supported = Some(true);
    }
    if supported.is_none() && levels.is_empty() && mappings.is_empty() {
        return None;
    }
    Some(ProviderCatalogReasoning {
        supported,
        levels,
        mappings,
    })
}

fn collect_reasoning_metadata(
    value: &Value,
    protocol: &ProviderProtocol,
    supported: &mut Option<bool>,
    levels: &mut Vec<ReasoningLevel>,
    mappings: &mut BTreeMap<ReasoningLevel, ReasoningMapping>,
) {
    match value {
        Value::Bool(value) => *supported = Some(*value),
        Value::String(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "enabled" | "supported" | "true" | "on" => *supported = Some(true),
                "disabled" | "unsupported" | "none" | "false" | "off" => *supported = Some(false),
                _ => add_reasoning_level(value, protocol, None, levels, mappings),
            }
        }
        Value::Number(value) => {
            if value.as_u64().is_some_and(|value| value > 0) {
                *supported = Some(true);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_reasoning_metadata(value, protocol, supported, levels, mappings);
            }
        }
        Value::Object(object) => {
            for key in [
                "levels",
                "supported_levels",
                "supportedLevels",
                "reasoning_levels",
                "reasoningLevels",
                "effort",
                "supported_efforts",
                "supportedEfforts",
                "modes",
                "supported_modes",
                "supportedModes",
                "types",
            ] {
                if let Some(value) = object.get(key) {
                    collect_reasoning_metadata(value, protocol, supported, levels, mappings);
                }
            }
            for (key, value) in object {
                let Some(level) = normalize_reasoning_level(key) else {
                    continue;
                };
                let budget_tokens = parse_reasoning_budget(value);
                if value.as_bool() == Some(false) {
                    continue;
                }
                add_reasoning_level(key, protocol, budget_tokens, levels, mappings);
                if level != ReasoningLevel::Off {
                    *supported = Some(true);
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

fn add_reasoning_level(
    value: &str,
    protocol: &ProviderProtocol,
    budget_tokens: Option<u32>,
    levels: &mut Vec<ReasoningLevel>,
    mappings: &mut BTreeMap<ReasoningLevel, ReasoningMapping>,
) {
    let Some(level) = normalize_reasoning_level(value) else {
        return;
    };
    if !levels.contains(&level) {
        levels.push(level);
    }
    let Some(mapping) = reasoning_mapping(protocol, level, value, budget_tokens) else {
        return;
    };
    if budget_tokens.is_some() {
        mappings.insert(level, mapping);
    } else {
        mappings.entry(level).or_insert(mapping);
    }
}

fn normalize_reasoning_level(value: &str) -> Option<ReasoningLevel> {
    let normalized = value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-' && *character != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "off" | "none" => Some(ReasoningLevel::Off),
        "low" | "minimal" => Some(ReasoningLevel::Low),
        "medium" | "med" | "balanced" => Some(ReasoningLevel::Medium),
        "high" => Some(ReasoningLevel::High),
        "xhigh" | "extrahigh" => Some(ReasoningLevel::XHigh),
        "max" | "maximum" => Some(ReasoningLevel::Max),
        "auto" | "adaptive" => Some(ReasoningLevel::Auto),
        _ => None,
    }
}

fn parse_reasoning_budget(value: &Value) -> Option<u32> {
    if let Some(budget) = parse_positive_u32(value) {
        return Some(budget);
    }
    let object = value.as_object()?;
    [
        "budget_tokens",
        "budgetTokens",
        "thinking_budget",
        "thinkingBudget",
        "max_thinking_tokens",
        "maxThinkingTokens",
    ]
    .iter()
    .filter_map(|key| object.get(*key))
    .find_map(parse_positive_u32)
}

fn level_name(level: ReasoningLevel) -> &'static str {
    match level {
        ReasoningLevel::Off => "off",
        ReasoningLevel::Low => "low",
        ReasoningLevel::Medium => "medium",
        ReasoningLevel::High => "high",
        ReasoningLevel::XHigh => "xhigh",
        ReasoningLevel::Max => "max",
        ReasoningLevel::Auto => "auto",
    }
}

fn reasoning_mapping(
    protocol: &ProviderProtocol,
    level: ReasoningLevel,
    native_value: &str,
    budget_tokens: Option<u32>,
) -> Option<ReasoningMapping> {
    if let Some(budget_tokens) = budget_tokens {
        return Some(ReasoningMapping::BudgetTokens(budget_tokens));
    }
    if level == ReasoningLevel::Off {
        return Some(ReasoningMapping::Disabled);
    }
    if level == ReasoningLevel::Auto && matches!(protocol, ProviderProtocol::AnthropicMessages) {
        return Some(ReasoningMapping::Adaptive);
    }

    let native_value = if native_value.trim().is_empty() {
        level_name(level).to_string()
    } else {
        native_value.trim().to_ascii_lowercase()
    };
    match protocol {
        ProviderProtocol::AnthropicMessages => Some(ReasoningMapping::Effort(native_value)),
        ProviderProtocol::GeminiGenerateContent => {
            Some(ReasoningMapping::NativeLevel(native_value))
        }
        ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses => {
            Some(ReasoningMapping::Effort(native_value))
        }
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
    fn adds_cpa_catalog_version_only_for_cpa_endpoint() {
        let provider = catalog_provider("http://127.0.0.1:8317/v1/models?tenant=test".to_string());
        assert_eq!(
            catalog_models_url(&provider).unwrap().as_str(),
            "http://127.0.0.1:8317/v1/models?tenant=test&client_version=1"
        );

        let provider =
            catalog_provider("http://127.0.0.1:8317/v1/models?client_version=custom".to_string());
        assert_eq!(
            catalog_models_url(&provider).unwrap().as_str(),
            "http://127.0.0.1:8317/v1/models?client_version=custom"
        );

        let provider = catalog_provider("https://api.openai.com/v1/models".to_string());
        assert_eq!(
            catalog_models_url(&provider).unwrap().as_str(),
            "https://api.openai.com/v1/models"
        );

        let provider = catalog_provider("http://[::1]:8317/v1/models".to_string());
        assert_eq!(
            catalog_models_url(&provider).unwrap().as_str(),
            "http://[::1]:8317/v1/models?client_version=1"
        );
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
                    context_window: None,
                    input_token_limit: None,
                    output_token_limit: None,
                    reasoning: None,
                    ..ProviderCatalogModel::default()
                },
                ProviderCatalogModel {
                    id: "gpt-4.1".to_string(),
                    display_name: "GPT 4.1".to_string(),
                    context_window: None,
                    input_token_limit: None,
                    output_token_limit: None,
                    reasoning: None,
                    ..ProviderCatalogModel::default()
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
                context_window: None,
                input_token_limit: None,
                output_token_limit: None,
                reasoning: None,
                ..ProviderCatalogModel::default()
            }]
        );
    }

    #[test]
    fn parses_cpa_catalog_models_identified_by_slug() {
        let models = parse_catalog_models_with_context(
            &json!({
                "models": [{
                    "slug": "gpt-5.6-sol",
                    "display_name": "GPT 5.6 Sol",
                    "context_window": 372_000,
                    "max_tokens": 128_000,
                    "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}]
                }]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
            true,
        );

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.6-sol");
        assert_eq!(models[0].display_name, "GPT 5.6 Sol");
        assert_eq!(models[0].context_window, Some(372_000));
        assert_eq!(models[0].max_context_window, None);
        assert_eq!(models[0].input_token_limit, Some(372_000));
        assert_eq!(models[0].output_token_limit, Some(128_000));
        assert_eq!(
            models[0]
                .reasoning
                .as_ref()
                .map(|reasoning| reasoning.levels.clone()),
            Some(vec![ReasoningLevel::Low, ReasoningLevel::High])
        );
    }

    #[test]
    fn parses_model_metadata_maps_using_the_object_key_as_model_id() {
        let models = parse_catalog_models_with_context(
            &json!({
                "models": {
                    "gpt-5.6-sol": {
                        "display_name": "GPT 5.6 Sol",
                        "context_window": 372_000,
                        "max_tokens": 128_000,
                        "reasoning": ["low", "high"]
                    }
                }
            }),
            &ProviderProtocol::OpenaiChatCompletions,
            true,
        );

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.6-sol");
        assert_eq!(models[0].input_token_limit, Some(372_000));
        assert_eq!(models[0].output_token_limit, Some(128_000));
    }

    #[test]
    fn parses_model_specific_token_limits_and_context_window() {
        let gemini = parse_catalog_models(
            &json!({
                "models": [{
                    "name": "models/gemini-2.5-pro",
                    "inputTokenLimit": 1_000_000,
                    "outputTokenLimit": 65_536
                }]
            }),
            &ProviderProtocol::GeminiGenerateContent,
        );
        assert_eq!(gemini[0].input_token_limit, Some(1_000_000));
        assert_eq!(gemini[0].output_token_limit, Some(65_536));

        let claude = parse_catalog_models(
            &json!({
                "data": [{
                    "id": "claude-sonnet",
                    "max_input_tokens": "200000",
                    "max_tokens": 32_000,
                    "context_length": 200_000
                }]
            }),
            &ProviderProtocol::AnthropicMessages,
        );
        assert_eq!(claude[0].input_token_limit, Some(200_000));
        assert_eq!(claude[0].output_token_limit, Some(32_000));

        let ambiguous = parse_catalog_models(
            &json!({
                "data": [{"id": "unknown", "context_length": 1_000_000}]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(ambiguous[0].context_window, None);
        assert_eq!(ambiguous[0].context_length, Some(1_000_000));
        assert_eq!(ambiguous[0].input_token_limit, None);
        assert_eq!(ambiguous[0].output_token_limit, None);

        let max_context = parse_catalog_models(
            &json!({
                "data": [{"id": "max-context", "max_context_window": 131_072}]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(max_context[0].context_window, Some(131_072));
        assert_eq!(max_context[0].max_context_window, Some(131_072));
    }

    #[test]
    fn preserves_complete_catalog_metadata_and_uses_cpa_context_as_input() {
        let cpa = parse_catalog_models_with_context(
            &json!({
                "data": [{
                    "id": "claude-sonnet",
                    "context_length": 1_000_000,
                    "max_tokens": 128_000,
                    "token_budget": 65_536,
                    "thinking": {"supported": true},
                    "capabilities": {"tools": true, "reasoning": true}
                }]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
            true,
        );
        assert_eq!(cpa[0].context_length, Some(1_000_000));
        assert_eq!(cpa[0].input_token_limit, Some(1_000_000));
        assert_eq!(cpa[0].output_token_limit, Some(128_000));
        assert_eq!(cpa[0].max_tokens, Some(128_000));
        assert_eq!(cpa[0].token_budget, Some(65_536));
        assert_eq!(cpa[0].thinking, Some(json!({"supported": true})));
        assert_eq!(
            cpa[0].capabilities,
            Some(json!({"tools": true, "reasoning": true}))
        );

        let openai = parse_catalog_models(
            &json!({"data": [{"id": "plain", "max_tokens": 8_192}]}),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(openai[0].max_tokens, Some(8_192));
        assert_eq!(openai[0].output_token_limit, None);

        let mistral = parse_catalog_models(
            &json!({"data": [{"id": "mistral-large", "max_context_length": 131_072}]}),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(mistral[0].context_length, Some(131_072));
        assert_eq!(mistral[0].input_token_limit, None);
    }

    #[test]
    fn parses_vendor_token_and_reasoning_metadata() {
        let anthropic = parse_catalog_models(
            &json!({
                "data": [{
                    "id": "claude-sonnet",
                    "max_input_tokens": 200_000,
                    "max_tokens": 8_192,
                    "capabilities": {
                        "thinking": {"supported": true},
                        "effort": {"supported_efforts": ["low", "high"]}
                    }
                }]
            }),
            &ProviderProtocol::AnthropicMessages,
        );
        assert_eq!(anthropic[0].input_token_limit, Some(200_000));
        assert_eq!(anthropic[0].output_token_limit, Some(8_192));
        assert_eq!(
            anthropic[0].reasoning.as_ref().unwrap().mappings,
            BTreeMap::from([
                (
                    ReasoningLevel::Low,
                    ReasoningMapping::Effort("low".to_string())
                ),
                (
                    ReasoningLevel::High,
                    ReasoningMapping::Effort("high".to_string())
                ),
            ])
        );

        let gemini = parse_catalog_models(
            &json!({
                "models": [{
                    "name": "models/gemini-2.5-pro",
                    "inputTokenLimit": 1_000_000,
                    "outputTokenLimit": 65_536,
                    "thinking": true
                }]
            }),
            &ProviderProtocol::GeminiGenerateContent,
        );
        assert_eq!(
            gemini[0].reasoning.as_ref().unwrap().mappings,
            BTreeMap::new()
        );

        let openrouter = parse_catalog_models(
            &json!({
                "data": [{
                    "id": "router-model",
                    "context_length": 131_072,
                    "top_provider": {
                        "context_length": 114_688,
                        "max_completion_tokens": 4_096
                    },
                    "reasoning": {
                        "supported_efforts": ["minimal", "high"]
                    }
                }]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(openrouter[0].context_window, None);
        assert_eq!(openrouter[0].context_length, Some(131_072));
        assert_eq!(openrouter[0].input_token_limit, None);
        assert_eq!(openrouter[0].output_token_limit, Some(4_096));
        assert_eq!(
            openrouter[0].reasoning.as_ref().unwrap().mappings,
            BTreeMap::from([
                (
                    ReasoningLevel::Low,
                    ReasoningMapping::Effort("minimal".to_string())
                ),
                (
                    ReasoningLevel::High,
                    ReasoningMapping::Effort("high".to_string())
                ),
            ])
        );

        let invalid = parse_catalog_models(
            &json!({
                "data": [{
                    "id": "invalid",
                    "max_input_tokens": 0,
                    "max_output_tokens": "not-a-number",
                    "max_tokens": -1
                }]
            }),
            &ProviderProtocol::OpenaiChatCompletions,
        );
        assert_eq!(invalid[0].input_token_limit, None);
        assert_eq!(invalid[0].output_token_limit, None);
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
                mappings: BTreeMap::from([
                    (
                        ReasoningLevel::Low,
                        ReasoningMapping::Effort("low".to_string())
                    ),
                    (
                        ReasoningLevel::High,
                        ReasoningMapping::Effort("high".to_string())
                    ),
                    (
                        ReasoningLevel::XHigh,
                        ReasoningMapping::Effort("xhigh".to_string())
                    ),
                ]),
            })
        );
        assert_eq!(models[1].reasoning, None);
        assert_eq!(
            models[2].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(false),
                levels: Vec::new(),
                mappings: BTreeMap::new(),
            })
        );
        assert_eq!(
            models[3].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(true),
                levels: Vec::new(),
                mappings: BTreeMap::new(),
            })
        );
        assert_eq!(
            models[4].reasoning,
            Some(ProviderCatalogReasoning {
                supported: Some(true),
                levels: Vec::new(),
                mappings: BTreeMap::new(),
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
        let recorded = recorded.await.unwrap();
        assert_eq!(recorded.path_and_query, "/v1/models");
        assert_eq!(recorded.authorization.as_deref(), Some("Bearer sk-catalog"));
    }
}
