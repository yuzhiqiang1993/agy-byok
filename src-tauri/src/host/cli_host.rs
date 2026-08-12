use crate::host::{ClientConfigurationState, ClientIntegrationState};
use host_integration::CliIntegrationState;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub installed: bool,
    pub proxy_running: bool,
    pub integration_state: ClientIntegrationState,
    pub configuration_state: ClientConfigurationState,
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
        CliIntegrationState::Managed => ClientIntegrationState::Managed,
        CliIntegrationState::External => ClientIntegrationState::External,
        CliIntegrationState::Mismatch => ClientIntegrationState::Mismatch,
        CliIntegrationState::Disabled => ClientIntegrationState::Official,
    };

    let configuration_state = cli_configuration_status(status.state, proxy_running);

    let can_enable_integration = status.installed
        && matches!(
            status.state,
            CliIntegrationState::Disabled
                | CliIntegrationState::Mismatch
                | CliIntegrationState::External
                | CliIntegrationState::Managed
        );
    let can_disable_integration =
        status.has_ownership || status.state != CliIntegrationState::Disabled;

    Ok(CliStatus {
        installed: status.installed,
        proxy_running,
        integration_state,
        configuration_state,
        can_enable_integration,
        can_disable_integration,
    })
}

fn cli_configuration_status(
    state: CliIntegrationState,
    proxy_running: bool,
) -> ClientConfigurationState {
    match state {
        CliIntegrationState::Managed | CliIntegrationState::External if !proxy_running => {
            ClientConfigurationState::ServiceStopped
        }
        CliIntegrationState::Managed | CliIntegrationState::External => {
            ClientConfigurationState::Matched
        }
        CliIntegrationState::Mismatch => ClientConfigurationState::NeedsUpdate,
        CliIntegrationState::Disabled => ClientConfigurationState::NotEnabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_cli_configuration_uses_declared_matched_state() {
        let state = cli_configuration_status(CliIntegrationState::External, true);

        assert_eq!(state, ClientConfigurationState::Matched);
    }
}
