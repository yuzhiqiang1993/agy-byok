mod parser;

use crate::domain::{
    AppConfig, ErrorCategory, Provider, ProxyError, ReasoningLevel, ReasoningMapping,
};
use crate::providers::get_adapter;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use parser::parse_catalog_models_with_context;
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

const CATALOG_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderCatalogReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) supported: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) levels: Vec<ReasoningLevel>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) mappings: BTreeMap<ReasoningLevel, ReasoningMapping>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogModel {
    pub(crate) id: String,
    pub(crate) display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auto_compact_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) input_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning: Option<ProviderCatalogReasoning>,
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
    .map_err(|error| ProxyError::new(ErrorCategory::InvalidRequest, error.to_string(), 400))?;

    if provider.models_endpoint.trim().is_empty() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            "模型列表地址不能为空",
            400,
        ));
    }

    let timeout_ms = provider.request_timeout_ms.min(CATALOG_TIMEOUT_MS);
    let connect_timeout_ms = provider.connect_timeout_ms.min(timeout_ms);
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
mod tests;
