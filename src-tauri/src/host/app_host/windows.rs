use super::{environment_integration_details, IntegrationDetails};
use crate::host::process::{launch_application_with_environment, terminate_process};
use crate::platform::AppPaths;
use host_integration::windows_environment::{self, WindowsEnvironmentOwner};
use std::path::Path;

pub(super) fn inspect_integration(
    integration_root: &Path,
    endpoint: &str,
) -> Result<IntegrationDetails, String> {
    let status =
        windows_environment::inspect(integration_root).map_err(|error| error.to_string())?;
    Ok(environment_integration_details(
        status.configured_endpoint.as_deref(),
        status.is_active_for(WindowsEnvironmentOwner::App),
        status.has_owner(WindowsEnvironmentOwner::App),
        endpoint,
    ))
}

pub(super) fn enable_integration(integration_root: &Path, endpoint: &str) -> Result<(), String> {
    windows_environment::enable(integration_root, WindowsEnvironmentOwner::App, endpoint)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn disable_integration(integration_root: &Path) -> Result<(), String> {
    windows_environment::disable(integration_root, WindowsEnvironmentOwner::App)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn request_app_exit(paths: &AppPaths) -> Result<(), String> {
    terminate_process(&paths.executable, "Antigravity")
}

pub(super) fn launch(paths: &AppPaths, endpoint: Option<&str>) -> Result<(), String> {
    let environment: Vec<_> = endpoint
        .map(|ep| ("CLOUD_CODE_URL", ep))
        .into_iter()
        .collect();
    launch_application_with_environment(
        &paths.installation,
        &paths.executable,
        "Antigravity App",
        &environment,
    )
}
