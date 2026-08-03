mod discovery;
mod ownership;
mod patch;
mod transaction;

#[cfg(test)]
mod tests;

use serde::Serialize;
use std::path::PathBuf;

pub const DEFAULT_ANTIGRAVITY_APP_PATH: &str = "/Applications/Antigravity.app";
pub const TARGET_OFFICIAL_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";

pub(super) const APP_INTEGRATION_SCHEMA_VERSION: u32 = 1;
pub(super) const RECEIPT_FILE_NAME: &str = ".agy-byok-language-server.json";
pub(super) const WRAPPER_MARKER: &str = "# AGY-BYOK-MANAGED-LANGUAGE-SERVER v1";
pub(super) const ENDPOINT_MARKER: &str = "# AGY-BYOK-ENDPOINT: ";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppIntegrationState {
    Disabled,
    Managed,
    Mismatch,
    Conflict,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppIntegrationStatus {
    pub state: AppIntegrationState,
    pub app_path: PathBuf,
    pub endpoint_matches: bool,
    pub configured_endpoint: Option<String>,
    pub app_version: Option<String>,
    pub original_sha256: Option<String>,
    pub message: String,
}

pub fn inspect_app_integration(
    app_path: impl AsRef<std::path::Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, crate::error::HostIntegrationError> {
    discovery::inspect_app_integration(app_path, endpoint)
}

pub fn enable_app_integration(
    app_path: impl AsRef<std::path::Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, crate::error::HostIntegrationError> {
    transaction::enable_app_integration(app_path, endpoint)
}

pub fn disable_app_integration(
    app_path: impl AsRef<std::path::Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, crate::error::HostIntegrationError> {
    transaction::disable_app_integration(app_path, endpoint)
}
