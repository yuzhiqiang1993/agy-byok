#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliIntegrationState {
    Disabled,
    Managed,
    Mismatch,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliIntegrationStatus {
    pub installed: bool,
    pub state: CliIntegrationState,
    pub has_ownership: bool,
}

pub fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    current::inspect_cli_integration(integration_root, target_endpoint)
}

pub fn detect_cli_executable() -> Option<PathBuf> {
    current::detect_cli_path()
}

pub fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    current::enable_cli_integration(integration_root, target_endpoint)
}

pub fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, crate::error::HostIntegrationError> {
    current::disable_cli_integration(integration_root, target_endpoint)
}

#[cfg(target_os = "macos")]
use macos as current;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use unsupported as current;
#[cfg(target_os = "windows")]
use windows as current;
