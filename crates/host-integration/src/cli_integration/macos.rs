use super::{CliIntegrationState, CliIntegrationStatus};
use crate::error::HostIntegrationError;
use crate::macos_environment::{self, MacOsEnvironmentOwner, MacOsEnvironmentStatus};
use std::path::{Path, PathBuf};

pub(super) fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let environment = macos_environment::inspect(integration_root)?;
    Ok(status_from_environment(environment, target_endpoint))
}

pub(super) fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let environment = macos_environment::enable(
        integration_root,
        MacOsEnvironmentOwner::Cli,
        target_endpoint,
    )?;
    Ok(status_from_environment(environment, target_endpoint))
}

pub(super) fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let environment = macos_environment::disable(integration_root, MacOsEnvironmentOwner::Cli)?;
    Ok(status_from_environment(environment, target_endpoint))
}

fn status_from_environment(
    environment: MacOsEnvironmentStatus,
    target_endpoint: &str,
) -> CliIntegrationStatus {
    let owns_current_value = environment.is_active_for(MacOsEnvironmentOwner::Cli);
    let has_ownership = environment.has_owner(MacOsEnvironmentOwner::Cli);
    let state = classify_state(
        environment.configured_endpoint.as_deref(),
        owns_current_value,
        target_endpoint,
    );
    CliIntegrationStatus {
        installed: detect_cli_path().is_some(),
        state,
        has_ownership,
    }
}

fn classify_state(
    configured_endpoint: Option<&str>,
    owns_current_value: bool,
    target_endpoint: &str,
) -> CliIntegrationState {
    match configured_endpoint {
        Some(endpoint) if endpoint == target_endpoint && owns_current_value => {
            CliIntegrationState::Managed
        }
        Some(endpoint) if endpoint == target_endpoint => CliIntegrationState::External,
        Some(_) => CliIntegrationState::Mismatch,
        None => CliIntegrationState::Disabled,
    }
}

pub(super) fn detect_cli_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    if let Some(path) = home.map(|home| home.join(".local/bin/agy")) {
        if path.is_file() {
            return Some(path);
        }
    }

    for candidate in ["/usr/local/bin/agy", "/opt/homebrew/bin/agy"] {
        let candidate = PathBuf::from(candidate);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join("agy"))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_distinguishes_managed_external_and_mismatch_values() {
        let target = "http://127.0.0.1:51234";

        assert_eq!(
            classify_state(Some(target), true, target),
            CliIntegrationState::Managed
        );
        assert_eq!(
            classify_state(Some(target), false, target),
            CliIntegrationState::External
        );
        assert_eq!(
            classify_state(Some("http://127.0.0.1:54321"), true, target),
            CliIntegrationState::Mismatch
        );
        assert_eq!(
            classify_state(None, false, target),
            CliIntegrationState::Disabled
        );
    }
}
