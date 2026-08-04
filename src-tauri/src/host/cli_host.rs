use host_integration::CliIntegrationState;
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
        CliIntegrationState::Disabled => "official",
    };

    let (configuration_state, configuration_message) = match status.state {
        CliIntegrationState::Managed if !proxy_running => (
            "service_stopped",
            "CLI 已配置代理模式，但本地代理未运行，请先启动本地代理".to_string(),
        ),
        CliIntegrationState::Managed => (
            "matched",
            "CLI 代理配置正常，当前已连接本地代理".to_string(),
        ),
        CliIntegrationState::External if !proxy_running => (
            "service_stopped",
            "CLI 已配置代理模式，但本地代理未运行，请先启动本地代理".to_string(),
        ),
        CliIntegrationState::External => (
            "external",
            "CLI 通过外部 CLOUD_CODE_URL 连接代理".to_string(),
        ),
        CliIntegrationState::Mismatch => (
            "needs_update",
            "CLI 代理配置与当前代理端口不匹配，请重新设置".to_string(),
        ),
        CliIntegrationState::Disabled => (
            "not_enabled",
            "CLI 当前使用官方配置，可随时启用代理模式".to_string(),
        ),
    };

    let can_enable_integration = status.installed
        && matches!(
            status.state,
            CliIntegrationState::Disabled
                | CliIntegrationState::Mismatch
                | CliIntegrationState::Managed
        );
    let can_disable_integration = status.state == CliIntegrationState::Managed
        || (status.state == CliIntegrationState::Mismatch
            && (status.has_ownership || !status.shell_configs_updated.is_empty()));

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
