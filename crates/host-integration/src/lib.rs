mod atomic_file;
mod cli_integration;
mod error;
mod ide_settings;
mod local_endpoint;
#[cfg(target_os = "macos")]
pub mod macos_environment;
mod serde_helpers;

#[cfg(target_os = "windows")]
pub mod windows_environment;

pub use cli_integration::{
    detect_cli_executable, disable_cli_integration, enable_cli_integration,
    inspect_cli_integration, CliIntegrationState, CliIntegrationStatus,
};
pub use error::HostIntegrationError;
pub use ide_settings::{
    disable_ide_settings, enable_ide_settings, inspect_ide_settings, IdeSettingsState,
    IdeSettingsStatus,
};
