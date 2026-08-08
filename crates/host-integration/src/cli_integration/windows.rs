use super::{CliIntegrationState, CliIntegrationStatus};
use crate::error::HostIntegrationError;
use crate::windows_environment::{self, WindowsEnvironmentOwner};
use std::path::{Path, PathBuf};

pub(super) fn detect_cli_path() -> Option<PathBuf> {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let official_path = official_cli_path(Path::new(&local_app_data));
        if official_path.is_file() {
            return Some(official_path);
        }
    }

    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|directory| directory.join("agy.exe"))
        .find(|candidate| candidate.is_file())
}

pub(super) fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let cli_path = detect_cli_path();
    let environment = windows_environment::inspect(integration_root)?;
    let has_ownership = environment.has_owner(WindowsEnvironmentOwner::Cli);
    let owns_current_value = environment.is_active_for(WindowsEnvironmentOwner::Cli);
    let configured_endpoint = environment.configured_endpoint;

    let state = match configured_endpoint.as_deref() {
        Some(endpoint) if endpoint == target_endpoint && owns_current_value => {
            CliIntegrationState::Managed
        }
        Some(endpoint) if endpoint == target_endpoint => CliIntegrationState::External,
        Some(_) => CliIntegrationState::Mismatch,
        None => CliIntegrationState::Disabled,
    };

    Ok(CliIntegrationStatus {
        installed: cli_path.is_some(),
        state,
        has_ownership,
    })
}

pub(super) fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    windows_environment::enable(
        integration_root,
        WindowsEnvironmentOwner::Cli,
        target_endpoint,
    )?;
    inspect_cli_integration(integration_root, target_endpoint)
}

pub(super) fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    windows_environment::disable(integration_root, WindowsEnvironmentOwner::Cli)?;
    inspect_cli_integration(integration_root, target_endpoint)
}

fn official_cli_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("agy").join("bin").join("agy.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_cli_path_uses_local_app_data_layout() {
        assert_eq!(
            official_cli_path(Path::new(r"C:\Users\demo\AppData\Local")),
            PathBuf::from(r"C:\Users\demo\AppData\Local\agy\bin\agy.exe")
        );
    }
}
