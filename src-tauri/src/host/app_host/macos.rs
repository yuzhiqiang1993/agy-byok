use super::{environment_integration_details, IntegrationDetails};
use crate::host::process::{
    launch_application_with_environment, terminate_process, wait_for_process_state,
};
use crate::platform::AppPaths;
use host_integration::macos_environment::{self, MacOsEnvironmentOwner};
use std::path::Path;
use std::process::Command;

pub(super) fn inspect_integration(
    integration_root: &Path,
    endpoint: &str,
) -> Result<IntegrationDetails, String> {
    let status = macos_environment::inspect(integration_root).map_err(|error| error.to_string())?;
    Ok(environment_integration_details(
        status.configured_endpoint.as_deref(),
        status.is_active_for(MacOsEnvironmentOwner::App),
        status.has_owner(MacOsEnvironmentOwner::App),
        endpoint,
    ))
}

pub(super) fn enable_integration(integration_root: &Path, endpoint: &str) -> Result<(), String> {
    macos_environment::enable(integration_root, MacOsEnvironmentOwner::App, endpoint)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn disable_integration(integration_root: &Path, _endpoint: &str) -> Result<(), String> {
    macos_environment::disable(integration_root, MacOsEnvironmentOwner::App)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn request_app_exit(paths: &AppPaths) -> Result<(), String> {
    let _ = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application id \"com.google.antigravity\" to quit",
        ])
        .status();
    if let Err(error) = wait_for_process_state(&paths.executable, "Antigravity", false) {
        terminate_process(&paths.executable, "Antigravity")
            .map_err(|force_error| format!("{error}；强制结束 Antigravity 失败：{force_error}"))?;
    }
    Ok(())
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
