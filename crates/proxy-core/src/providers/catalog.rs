mod parser;

use crate::domain::{
    AppConfig, ErrorCategory, Provider, ProxyError, ReasoningLevel, ReasoningMapping,
};
use crate::providers::get_adapter;
use crate::upstream_body::{read_limited_response_body, DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES};
use parser::{parse_catalog_models_with_context, parse_official_catalog_models};
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

const CATALOG_TIMEOUT_MS: u64 = 15_000;
const OFFICIAL_LANGUAGE_SERVER_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogReasoning {
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
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_token_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ProviderCatalogReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_compression: Option<UpstreamCompressionPolicy>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamCompressionPolicy {
    pub enabled: bool,
    pub token_threshold: u32,
    pub max_token_limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_model: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialCatalogSource {
    Ide,
    App,
}

impl OfficialCatalogSource {
    const fn label(self) -> &'static str {
        match self {
            Self::Ide => "Antigravity IDE",
            Self::App => "Antigravity App",
        }
    }
}

pub async fn fetch_official_models_catalog(
    source: OfficialCatalogSource,
) -> Result<Vec<ProviderCatalogModel>, ProxyError> {
    let candidates = language_server_candidates(source).await?;
    if candidates.is_empty() {
        return Err(ProxyError::new(
            ErrorCategory::InvalidRequest,
            format!("未找到 {} 后台语言服务", source.label()),
            404,
        ));
    }

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(OFFICIAL_LANGUAGE_SERVER_TIMEOUT)
        .timeout(OFFICIAL_LANGUAGE_SERVER_TIMEOUT)
        .build()
        .map_err(|e| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("创建探针客户端失败: {e}"),
                500,
            )
        })?;

    let mut last_error = None;
    for (port, csrf) in candidates {
        let url = format!(
            "https://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/GetAvailableModels"
        );
        let response = match client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-Codeium-Csrf-Token", csrf)
            .body("{}")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(ProxyError::new(
                    ErrorCategory::UpstreamServerError,
                    format!("连接 {} 语言服务失败: {error}", source.label()),
                    502,
                ));
                continue;
            }
        };
        let status = response.status().as_u16();
        let json_text = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                last_error = Some(ProxyError::new(
                    ErrorCategory::UpstreamServerError,
                    format!("读取 {} 语言服务响应失败: {error}", source.label()),
                    502,
                ));
                continue;
            }
        };
        if !(200..300).contains(&status) {
            last_error = Some(ProxyError::new(
                ErrorCategory::UpstreamServerError,
                format!("{} 语言服务返回 HTTP {status}", source.label()),
                502,
            ));
            continue;
        }
        let parsed: Value = match serde_json::from_str(&json_text) {
            Ok(parsed) => parsed,
            Err(error) => {
                last_error = Some(ProxyError::new(
                    ErrorCategory::UpstreamServerError,
                    format!("解析 {} 语言服务响应失败: {error}", source.label()),
                    502,
                ));
                continue;
            }
        };
        let result = parse_official_catalog_models(&parsed);
        if !result.is_empty() {
            return Ok(result);
        }
        last_error = Some(ProxyError::new(
            ErrorCategory::UpstreamServerError,
            format!("{} 语言服务响应中没有有效的 models 节点", source.label()),
            502,
        ));
    }
    Err(last_error.unwrap_or_else(|| {
        ProxyError::new(
            ErrorCategory::UpstreamServerError,
            format!("没有可连接的 {} 语言服务端口", source.label()),
            502,
        )
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct LanguageServerProcess {
    pid: u32,
    source: OfficialCatalogSource,
    csrf: String,
    configured_port: Option<u16>,
}

fn command_flag_value<'a>(command: &'a str, flag: &str) -> Option<&'a str> {
    let prefix = format!("{flag}=");
    let mut parts = command.split_whitespace();
    while let Some(part) = parts.next() {
        if part == flag {
            return parts.next().map(|value| value.trim_matches('"'));
        }
        if let Some(value) = part.strip_prefix(&prefix) {
            return Some(value.trim_matches('"'));
        }
    }
    None
}

fn parse_language_server_processes(listing: &str) -> Vec<LanguageServerProcess> {
    listing
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let separator = line.find(char::is_whitespace)?;
            let pid = line[..separator].parse::<u32>().ok()?;
            let command = line[separator..].trim();
            if !command.contains("language_server") {
                return None;
            }
            let source = match command_flag_value(command, "--subclient_type") {
                Some("ide") => OfficialCatalogSource::Ide,
                Some("hub") => OfficialCatalogSource::App,
                _ if command_flag_value(command, "--https_server_port").is_some() => {
                    OfficialCatalogSource::App
                }
                _ => return None,
            };
            let configured_port = command_flag_value(command, "--https_server_port")
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|port| *port > 0);
            let csrf = command_flag_value(command, "--csrf_token")?.to_string();
            (!csrf.is_empty()).then_some(LanguageServerProcess {
                pid,
                source,
                csrf,
                configured_port,
            })
        })
        .collect()
}

fn parse_listening_ports(listing: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in listing.lines() {
        let endpoint = line.trim().strip_prefix('n').unwrap_or_else(|| line.trim());
        let Some(port) = endpoint
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        if port > 0 && !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports
}

#[cfg(not(target_os = "windows"))]
async fn language_server_process_listing() -> Result<String, ProxyError> {
    let output = tokio::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("读取进程列表失败: {error}"),
                500,
            )
        })?;
    if !output.status.success() {
        return Err(ProxyError::new(
            ErrorCategory::Internal,
            format!("读取进程列表失败: {}", output.status),
            500,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "windows")]
async fn language_server_process_listing() -> Result<String, ProxyError> {
    let script = r#"Get-CimInstance Win32_Process -Filter "Name LIKE '%language_server%'" | ForEach-Object { "$($_.ProcessId) $($_.CommandLine)" }"#;
    let output = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("读取 Windows 语言服务进程失败: {error}"),
                500,
            )
        })?;
    if !output.status.success() {
        return Err(ProxyError::new(
            ErrorCategory::Internal,
            format!("读取 Windows 语言服务进程失败: {}", output.status),
            500,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
async fn process_listening_ports(pid: u32) -> Result<Vec<u16>, ProxyError> {
    let output = tokio::process::Command::new("/usr/sbin/lsof")
        .args([
            "-nP",
            "-a",
            "-p",
            &pid.to_string(),
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fn",
        ])
        .output()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("读取语言服务监听端口失败: {error}"),
                500,
            )
        })?;
    Ok(parse_listening_ports(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(target_os = "windows")]
async fn process_listening_ports(pid: u32) -> Result<Vec<u16>, ProxyError> {
    let script = format!(
        "(Get-NetTCPConnection -State Listen -OwningProcess {pid} -ErrorAction SilentlyContinue).LocalPort"
    );
    let output = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("读取 Windows 语言服务监听端口失败: {error}"),
                500,
            )
        })?;
    Ok(parse_listening_ports(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn process_listening_ports(_pid: u32) -> Result<Vec<u16>, ProxyError> {
    Ok(Vec::new())
}

async fn language_server_candidates(
    source: OfficialCatalogSource,
) -> Result<Vec<(u16, String)>, ProxyError> {
    let listing = language_server_process_listing().await?;
    let processes = parse_language_server_processes(&listing)
        .into_iter()
        .filter(|process| process.source == source);
    let mut candidates: Vec<(u16, String)> = Vec::new();
    for process in processes {
        let mut ports = process.configured_port.into_iter().collect::<Vec<_>>();
        for port in process_listening_ports(process.pid).await? {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        for port in ports {
            if !candidates.iter().any(|candidate| candidate.0 == port) {
                candidates.push((port, process.csrf.clone()));
            }
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests;
