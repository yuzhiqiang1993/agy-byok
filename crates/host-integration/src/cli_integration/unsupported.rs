use super::{CliIntegrationState, CliIntegrationStatus};
use crate::error::HostIntegrationError;
use std::path::Path;

pub(super) fn inspect_cli_integration(
    _integration_root: impl AsRef<Path>,
    _target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    Ok(CliIntegrationStatus {
        installed: false,
        state: CliIntegrationState::Disabled,
        has_ownership: false,
    })
}

pub(super) fn enable_cli_integration(
    _integration_root: impl AsRef<Path>,
    _target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    Err(HostIntegrationError::InvalidIntegration(
        "当前平台不支持 Antigravity CLI 接入".to_string(),
    ))
}

pub(super) fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    inspect_cli_integration(integration_root, target_endpoint)
}
