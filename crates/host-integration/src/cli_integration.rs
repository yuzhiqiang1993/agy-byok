mod discovery;
mod ownership;
mod patch;
mod transaction;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CLI_MARKER_BEGIN: &str = "# >>> AGY BYOK CLI Integration >>>";
pub const CLI_MARKER_END: &str = "# <<< AGY BYOK CLI Integration <<<";
pub const CLI_FISH_MARKER_BEGIN: &str = "# >>> AGY BYOK CLI Integration (Fish) >>>";
pub const CLI_FISH_MARKER_END: &str = "# <<< AGY BYOK CLI Integration (Fish) <<<";
pub const CLI_OWNERSHIP_FILE: &str = "cli-ownership.json";
pub(super) const OWNERSHIP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CliIntegrationState {
    Disabled,
    Managed,
    Mismatch,
    External,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CliIntegrationStatus {
    pub installed: bool,
    pub state: CliIntegrationState,
    pub cli_path: Option<PathBuf>,
    pub configured_endpoint: Option<String>,
    pub has_ownership: bool,
    pub endpoint_matches: bool,
    pub shell_configs_updated: Vec<PathBuf>,
    pub message: String,
}

pub fn user_home_dir() -> Option<PathBuf> {
    discovery::user_home_dir()
}

pub fn detect_cli_path() -> Option<PathBuf> {
    discovery::detect_cli_path()
}

pub fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    discovery::inspect_cli_integration(integration_root, target_endpoint)
}

pub fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    transaction::enable_cli_integration(integration_root, target_endpoint)
}

pub fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    transaction::disable_cli_integration(integration_root, target_endpoint)
}
