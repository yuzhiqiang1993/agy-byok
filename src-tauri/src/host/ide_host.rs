use crate::host::process::{is_process_running, launch_application, wait_for_process_state};
use crate::host::{ClientConfigurationState, ClientIntegrationState};
use crate::platform::IdePaths;
use host_integration::{inspect_ide_settings, IdeSettingsState};
use serde::Serialize;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeStatus {
    pub installed: bool,
    pub compatible: bool,
    pub ide_running: bool,
    pub proxy_running: bool,
    pub integration_state: ClientIntegrationState,
    pub settings_path: String,
    pub configuration_state: ClientConfigurationState,
    pub can_enable_integration: bool,
    pub can_launch_ide: bool,
    pub can_disable_integration: bool,
}

pub fn discover_ide_sync(
    paths: Option<&IdePaths>,
    integration_root: &Path,
    endpoint: &str,
    proxy_running: bool,
) -> Result<IdeStatus, String> {
    let Some(paths) = paths else {
        return Ok(unavailable_status(proxy_running));
    };
    let installed = paths.installation.is_dir() && paths.executable.is_file();
    let executable = executable_path(paths);
    let ide_running = installed && is_process_running(&executable, "Antigravity IDE")?;

    let (integration_state, endpoint_matches, can_disable, settings_valid) =
        match paths.settings.as_deref() {
            Some(settings_path) => {
                match inspect_ide_settings(settings_path, integration_root, endpoint) {
                    Ok(status) => match status.state {
                        IdeSettingsState::Disabled => {
                            (ClientIntegrationState::Official, false, false, true)
                        }
                        IdeSettingsState::Managed => (
                            if status.endpoint_matches {
                                ClientIntegrationState::Managed
                            } else {
                                ClientIntegrationState::Mismatch
                            },
                            status.endpoint_matches,
                            true,
                            true,
                        ),
                        IdeSettingsState::External => (
                            if status.endpoint_matches {
                                ClientIntegrationState::External
                            } else {
                                ClientIntegrationState::Mismatch
                            },
                            status.endpoint_matches,
                            false,
                            true,
                        ),
                    },
                    Err(_) => (ClientIntegrationState::Conflict, false, false, false),
                }
            }
            None => (ClientIntegrationState::Unavailable, false, false, false),
        };

    let configuration_state = ide_configuration_status(
        integration_state,
        endpoint_matches,
        proxy_running,
        ide_running,
    );
    let integration_ready = integration_state.is_ready();
    let can_enable_integration = installed
        && settings_valid
        && matches!(
            integration_state,
            ClientIntegrationState::Official
                | ClientIntegrationState::Mismatch
                | ClientIntegrationState::Managed
        );
    let can_launch_ide = installed
        && !ide_running
        && (integration_state == ClientIntegrationState::Official
            || (integration_ready && proxy_running));

    Ok(IdeStatus {
        installed,
        compatible: installed,
        ide_running,
        proxy_running,
        integration_state,
        settings_path: paths
            .settings
            .as_deref()
            .map(Path::display)
            .map(|path| path.to_string())
            .unwrap_or_default(),
        configuration_state,
        can_enable_integration,
        can_launch_ide,
        can_disable_integration: can_disable,
    })
}

pub fn stop_ide_for_reconfiguration(paths: &IdePaths) -> Result<bool, String> {
    let executable = executable_path(paths);
    let was_running = is_process_running(&executable, "Antigravity IDE")?;
    if !was_running {
        return Ok(false);
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "tell application id \"com.google.antigravity-ide\" to quit",
            ])
            .status()
            .map_err(|error| format!("无法请求 Antigravity IDE 退出：{error}"))?;
        if !status.success() {
            return Err(format!("请求 Antigravity IDE 退出失败：{status}"));
        }
        if let Err(error) = wait_for_process_state(&executable, "Antigravity IDE", false) {
            crate::host::process::terminate_process(&executable, "Antigravity IDE").map_err(
                |force_error| format!("{error}；强制结束 Antigravity IDE 失败：{force_error}"),
            )?;
        }
        Ok(true)
    }

    #[cfg(not(target_os = "macos"))]
    {
        crate::host::process::terminate_process(&executable, "Antigravity IDE")?;
        Ok(true)
    }
}

pub fn restart_ide(paths: &IdePaths) -> Result<(), String> {
    launch_ide(paths)?;
    wait_for_process_state(&executable_path(paths), "Antigravity IDE", true)
}

pub fn launch_ide(paths: &IdePaths) -> Result<(), String> {
    launch_application(
        &paths.installation,
        &executable_path(paths),
        "Antigravity IDE",
    )
}

fn executable_path(paths: &IdePaths) -> PathBuf {
    paths.executable.clone()
}

fn ide_configuration_status(
    integration_state: ClientIntegrationState,
    endpoint_matches: bool,
    proxy_running: bool,
    ide_running: bool,
) -> ClientConfigurationState {
    match integration_state {
        ClientIntegrationState::Official => ClientConfigurationState::NotEnabled,
        ClientIntegrationState::Conflict | ClientIntegrationState::Unavailable => {
            ClientConfigurationState::Unavailable
        }
        _ if !endpoint_matches => ClientConfigurationState::NeedsUpdate,
        _ if !proxy_running => ClientConfigurationState::ServiceStopped,
        _ if !ide_running => ClientConfigurationState::NotRunning,
        _ => ClientConfigurationState::Matched,
    }
}

fn unavailable_status(proxy_running: bool) -> IdeStatus {
    IdeStatus {
        installed: false,
        compatible: false,
        ide_running: false,
        proxy_running,
        integration_state: ClientIntegrationState::Unavailable,
        settings_path: String::new(),
        configuration_state: ClientConfigurationState::Unavailable,
        can_enable_integration: false,
        can_launch_ide: false,
        can_disable_integration: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_status_uses_the_settings_endpoint_on_every_platform() {
        assert_eq!(
            ide_configuration_status(ClientIntegrationState::Managed, true, true, true),
            ClientConfigurationState::Matched
        );
        assert_eq!(
            ide_configuration_status(ClientIntegrationState::Managed, false, true, true),
            ClientConfigurationState::NeedsUpdate
        );
        assert_eq!(
            ide_configuration_status(ClientIntegrationState::Managed, true, false, true),
            ClientConfigurationState::ServiceStopped
        );
    }
}
