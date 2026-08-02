use host_integration::{CliIntegrationState};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub installed: bool,
    pub proxy_running: bool,
    pub cli_path: Option<String>,
    pub integration_state: &'static str,
    pub integration_message: String,
    pub configuration_state: &'static str,
    pub configuration_message: String,
    pub configured_endpoint: Option<String>,
    pub can_enable_integration: bool,
    pub can_disable_integration: bool,
}

pub fn discover_cli_sync(
    integration_root: &Path,
    endpoint: &str,
    proxy_running: bool,
) -> Result<CliStatus, String> {
    let status = host_integration::inspect_cli_integration(integration_root, endpoint)
        .map_err(|e| e.to_string())?;

    let integration_state = match status.state {
        CliIntegrationState::Managed => "managed",
        CliIntegrationState::External => "external",
        CliIntegrationState::Mismatch => "mismatch",
        CliIntegrationState::Disabled => "disabled",
    };

    let (configuration_state, configuration_message) = match status.state {
        CliIntegrationState::Managed => (
            "matched",
            "CLI 客户端配置有效，当前连通本地代理".to_string(),
        ),
        CliIntegrationState::External => (
            "external",
            "CLI 客户端通过外部 CLOUD_CODE_URL 连接代理".to_string(),
        ),
        CliIntegrationState::Mismatch => (
            "needs_update",
            "CLI 客户端配置端口与当前代理端口不匹配".to_string(),
        ),
        CliIntegrationState::Disabled => {
            ("not_configured", "CLI 客户端未连接本地代理".to_string())
        }
    };

    let can_enable_integration = status.installed
        && (status.state == CliIntegrationState::Disabled
            || status.state == CliIntegrationState::Mismatch);
    let can_disable_integration = status.state == CliIntegrationState::Managed
        || status.state == CliIntegrationState::Mismatch;

    Ok(CliStatus {
        installed: status.installed,
        proxy_running,
        cli_path: status.cli_path.map(|p| p.to_string_lossy().to_string()),
        integration_state,
        integration_message: status.message,
        configuration_state,
        configuration_message,
        configured_endpoint: status.configured_endpoint,
        can_enable_integration,
        can_disable_integration,
    })
}
