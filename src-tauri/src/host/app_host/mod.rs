#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::host::process::{is_process_running, terminate_process, wait_for_process_state};
use crate::host::{ClientConfigurationState, ClientIntegrationState};
use crate::platform::AppPaths;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub installed: bool,
    pub app_running: bool,
    pub proxy_running: bool,
    pub integration_state: ClientIntegrationState,
    pub configuration_state: ClientConfigurationState,
    pub can_enable_integration: bool,
    pub can_launch_app: bool,
    pub can_disable_integration: bool,
}

pub(super) struct IntegrationDetails {
    pub state: ClientIntegrationState,
    pub endpoint_matches: bool,
    pub can_enable: bool,
    pub can_disable: bool,
}

fn environment_integration_details(
    configured_endpoint: Option<&str>,
    owns_current_value: bool,
    has_ownership: bool,
    target_endpoint: &str,
) -> IntegrationDetails {
    let endpoint_matches = configured_endpoint == Some(target_endpoint);
    let state = match configured_endpoint {
        Some(_) if owns_current_value && endpoint_matches => ClientIntegrationState::Managed,
        Some(_) if endpoint_matches => ClientIntegrationState::External,
        Some(_) => ClientIntegrationState::Mismatch,
        None => ClientIntegrationState::Official,
    };
    IntegrationDetails {
        state,
        endpoint_matches,
        can_enable: true,
        can_disable: has_ownership || state != ClientIntegrationState::Official,
    }
}

pub fn discover_app_sync(
    paths: Option<&AppPaths>,
    integration_root: &Path,
    endpoint: &str,
    proxy_running: bool,
) -> Result<AppStatus, String> {
    let Some(paths) = paths else {
        return Ok(unavailable_status(proxy_running));
    };
    let installed = paths.installation.is_dir()
        && paths.executable.is_file()
        && required_integration_files_exist(paths);
    if !installed {
        return Ok(not_installed_status(proxy_running));
    }

    let app_running = is_process_running(&paths.executable, "Antigravity")?;
    let integration = inspect_integration(integration_root, &paths.installation, endpoint)?;
    let configuration_state = configuration_state(
        integration.state,
        integration.endpoint_matches,
        proxy_running,
        app_running,
    );
    let integration_ready = integration.state.is_ready();

    Ok(AppStatus {
        installed,
        app_running,
        proxy_running,
        integration_state: integration.state,
        configuration_state,
        can_enable_integration: integration.can_enable,
        can_launch_app: integration.state == ClientIntegrationState::Official
            || (integration_ready && proxy_running),
        can_disable_integration: integration.can_disable,
    })
}

pub fn enable_integration(
    paths: &AppPaths,
    integration_root: &Path,
    endpoint: &str,
) -> Result<(), String> {
    platform_enable(integration_root, &paths.installation, endpoint)
}

pub fn disable_integration(
    paths: &AppPaths,
    integration_root: &Path,
    endpoint: &str,
) -> Result<(), String> {
    platform_disable(integration_root, &paths.installation, endpoint)
}

// 返回值只表示宿主主进程是否需要重启，残留语言服务不会触发重新打开 App。
pub fn stop_app_for_reconfiguration(paths: &AppPaths) -> Result<bool, String> {
    let app_was_running = is_process_running(&paths.executable, "Antigravity")?;
    if app_was_running {
        request_app_exit(paths)?;
    }

    let language_server_stopped = stop_language_server(paths, app_was_running)?;
    if app_was_running || language_server_stopped {
        std::thread::sleep(Duration::from_millis(800));
    }
    Ok(app_was_running)
}

pub fn restart_app(paths: &AppPaths, endpoint: Option<&str>) -> Result<(), String> {
    launch_app(paths, endpoint)?;
    wait_for_process_state(&paths.executable, "Antigravity", true)
}

pub fn launch_app(paths: &AppPaths, endpoint: Option<&str>) -> Result<(), String> {
    platform_launch(paths, endpoint)
}

fn stop_language_server(
    paths: &AppPaths,
    wait_for_graceful_shutdown: bool,
) -> Result<bool, String> {
    let language_server = &paths.language_server;
    if !language_server.is_file()
        || !is_process_running(language_server, "Antigravity Language Server")?
    {
        return Ok(false);
    }

    if wait_for_graceful_shutdown
        && wait_for_process_state(language_server, "Antigravity Language Server", false).is_ok()
    {
        return Ok(false);
    }
    terminate_process(language_server, "Antigravity Language Server")?;
    Ok(true)
}

fn configuration_state(
    integration_state: ClientIntegrationState,
    endpoint_matches: bool,
    proxy_running: bool,
    app_running: bool,
) -> ClientConfigurationState {
    match integration_state {
        ClientIntegrationState::Official => ClientConfigurationState::NotEnabled,
        ClientIntegrationState::Conflict | ClientIntegrationState::Unavailable => {
            ClientConfigurationState::Unavailable
        }
        _ if !endpoint_matches => ClientConfigurationState::NeedsUpdate,
        _ if !proxy_running => ClientConfigurationState::ServiceStopped,
        _ if !app_running => ClientConfigurationState::NotRunning,
        _ => ClientConfigurationState::Matched,
    }
}

#[cfg(target_os = "macos")]
fn required_integration_files_exist(paths: &AppPaths) -> bool {
    paths.language_server.is_file()
}

#[cfg(not(target_os = "macos"))]
fn required_integration_files_exist(_paths: &AppPaths) -> bool {
    true
}

#[cfg(target_os = "macos")]
fn inspect_integration(
    integration_root: &Path,
    _installation: &Path,
    endpoint: &str,
) -> Result<IntegrationDetails, String> {
    macos::inspect_integration(integration_root, endpoint)
}

#[cfg(target_os = "windows")]
fn inspect_integration(
    integration_root: &Path,
    _installation: &Path,
    endpoint: &str,
) -> Result<IntegrationDetails, String> {
    windows::inspect_integration(integration_root, endpoint)
}

#[cfg(target_os = "macos")]
fn platform_enable(
    integration_root: &Path,
    _installation: &Path,
    endpoint: &str,
) -> Result<(), String> {
    macos::enable_integration(integration_root, endpoint)
}

#[cfg(target_os = "windows")]
fn platform_enable(
    integration_root: &Path,
    _installation: &Path,
    endpoint: &str,
) -> Result<(), String> {
    windows::enable_integration(integration_root, endpoint)
}

#[cfg(target_os = "macos")]
fn platform_disable(
    integration_root: &Path,
    _installation: &Path,
    endpoint: &str,
) -> Result<(), String> {
    macos::disable_integration(integration_root, endpoint)
}

#[cfg(target_os = "windows")]
fn platform_disable(
    integration_root: &Path,
    _installation: &Path,
    _endpoint: &str,
) -> Result<(), String> {
    windows::disable_integration(integration_root)
}

#[cfg(target_os = "macos")]
fn request_app_exit(paths: &AppPaths) -> Result<(), String> {
    macos::request_app_exit(paths)
}

#[cfg(target_os = "windows")]
fn request_app_exit(paths: &AppPaths) -> Result<(), String> {
    windows::request_app_exit(paths)
}

#[cfg(target_os = "macos")]
fn platform_launch(paths: &AppPaths, endpoint: Option<&str>) -> Result<(), String> {
    macos::launch(paths, endpoint)
}

#[cfg(target_os = "windows")]
fn platform_launch(paths: &AppPaths, endpoint: Option<&str>) -> Result<(), String> {
    windows::launch(paths, endpoint)
}

fn not_installed_status(proxy_running: bool) -> AppStatus {
    AppStatus {
        installed: false,
        app_running: false,
        proxy_running,
        integration_state: ClientIntegrationState::Unavailable,
        configuration_state: ClientConfigurationState::Unavailable,
        can_enable_integration: false,
        can_launch_app: false,
        can_disable_integration: false,
    }
}

fn unavailable_status(proxy_running: bool) -> AppStatus {
    not_installed_status(proxy_running)
}

#[cfg(test)]
mod tests;
