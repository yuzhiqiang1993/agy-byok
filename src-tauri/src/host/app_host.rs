use crate::host::process::{
    command_argument, is_app_running, is_process_running, resolve_host_executable,
    terminate_process, wait_for_app_state, wait_for_process_state,
};
use host_integration::AppIntegrationState;
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub const ANTIGRAVITY_APP_PATH: &str = "/Applications/Antigravity.app";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub installed: bool,
    pub app_running: bool,
    pub proxy_running: bool,
    pub app_path: String,
    pub app_version: Option<String>,
    pub ls_path: String,
    pub integration_state: &'static str,
    pub integration_message: String,
    pub configuration_state: &'static str,
    pub configuration_message: String,
    pub configured_endpoint: Option<String>,
    pub can_enable_integration: bool,
    pub can_launch_app: bool,
    pub can_disable_integration: bool,
}

pub fn discover_app_sync(endpoint: &str, proxy_running: bool) -> Result<AppStatus, String> {
    let app_path = Path::new(ANTIGRAVITY_APP_PATH);
    let installed = app_path.is_dir();
    let app_running = is_app_running(app_path, "Antigravity")?;
    let mut app_version = None;
    let mut configured_endpoint = None;

    let (integration_state, integration_message, can_enable_integration, can_disable_integration) =
        if !installed {
            (
                "unavailable",
                "未检测到 Antigravity.app".to_string(),
                false,
                false,
            )
        } else {
            match host_integration::inspect_app_integration(app_path, endpoint) {
                Ok(status) => {
                    app_version = status.app_version;
                    configured_endpoint = status.configured_endpoint;
                    match status.state {
                        AppIntegrationState::Disabled => (
                            "official",
                            "官方模式：App 不使用本地代理；可以启用代理模式".to_string(),
                            true,
                            false,
                        ),
                        AppIntegrationState::Managed => (
                            "managed",
                            if proxy_running {
                                status.message
                            } else {
                                format!("{}；当前本地代理未运行", status.message)
                            },
                            true,
                            true,
                        ),
                        AppIntegrationState::Mismatch => ("mismatch", status.message, true, true),
                        AppIntegrationState::Conflict => ("conflict", status.message, false, false),
                    }
                }
                Err(error) => ("conflict", format!("检查失败：{error}"), false, false),
            }
        };

    let (configuration_state, configuration_message) = client_configuration_status(
        integration_state,
        proxy_running,
        app_running,
        app_path,
        endpoint,
    );
    let can_enable_integration = can_enable_integration
        || (integration_state == "managed" && configuration_state == "needs_update");
    let can_launch_app = installed
        && !app_running
        && (integration_state == "official" || (integration_state == "managed" && proxy_running));
    let ls_path = app_path
        .join("Contents/Resources/bin/language_server")
        .display()
        .to_string();

    Ok(AppStatus {
        installed,
        app_running,
        proxy_running,
        app_path: ANTIGRAVITY_APP_PATH.to_string(),
        app_version,
        ls_path,
        integration_state,
        integration_message,
        configuration_state,
        configuration_message,
        configured_endpoint,
        can_enable_integration,
        can_launch_app,
        can_disable_integration,
    })
}

// 返回值只表示宿主主进程是否需要重启，残留语言服务不会触发重新打开 App。
pub fn stop_app_for_reconfiguration(app_path: &Path, label: &str) -> Result<bool, String> {
    let app_was_running = is_app_running(app_path, label)?;
    if app_was_running {
        let script = if label == "Antigravity IDE" {
            format!(
                "tell application id \"{}\" to quit",
                crate::host::ide_host::ANTIGRAVITY_IDE_BUNDLE_ID
            )
        } else {
            format!("tell application \"{label}\" to quit")
        };
        let status = Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .status()
            .map_err(|error| format!("无法请求 {label} 退出：{error}"))?;
        if !status.success() {
            return Err(format!("请求 {label} 退出失败：{status}"));
        }

        if let Err(error) = wait_for_app_state(app_path, label, false) {
            terminate_process(&resolve_host_executable(app_path), label)
                .map_err(|force_error| format!("{error}；强制结束 {label} 失败：{force_error}"))?;
        }
    }

    // App 主进程退出后，仍可能残留由官方入口或 AGY BYOK Wrapper 启动的语言服务。
    let language_server_stopped = stop_app_language_servers(app_path, app_was_running)?;
    if app_was_running || language_server_stopped {
        std::thread::sleep(Duration::from_millis(800));
    }
    Ok(app_was_running)
}

fn stop_app_language_servers(
    app_path: &Path,
    wait_for_graceful_shutdown: bool,
) -> Result<bool, String> {
    let language_servers = [
        app_path.join("Contents/Resources/bin/language_server"),
        app_path.join("Contents/Resources/bin/language_server.real"),
    ];
    let mut stopped = false;
    for language_server in language_servers {
        if !language_server.is_file()
            || !is_process_running(&language_server, "Antigravity Language Server")?
        {
            continue;
        }

        if wait_for_graceful_shutdown {
            if let Err(error) =
                wait_for_process_state(&language_server, "Antigravity Language Server", false)
            {
                terminate_process(&language_server, "Antigravity Language Server").map_err(
                    |force_error| {
                        format!("{error}；强制结束 Antigravity Language Server 失败：{force_error}")
                    },
                )?;
                stopped = true;
            }
        } else {
            terminate_process(&language_server, "Antigravity Language Server")?;
            stopped = true;
        }
    }
    Ok(stopped)
}

pub fn restart_app_app(app_path: &Path) -> Result<(), String> {
    launch_app_app(app_path)?;
    wait_for_app_state(app_path, "Antigravity", true)
}

pub fn launch_app_app(app_path: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .env("TMPDIR", "/private/tmp")
        .arg(app_path)
        .status()
        .map_err(|error| format!("无法启动 Antigravity App：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("启动 Antigravity App 失败：{status}"))
    }
}

pub fn client_configuration_status(
    integration_state: &str,
    proxy_running: bool,
    client_running: bool,
    app_path: &Path,
    endpoint: &str,
) -> (&'static str, String) {
    match integration_state {
        "official" => (
            "not_enabled",
            "当前使用官方配置，可随时启用代理模式".to_string(),
        ),
        "mismatch" => ("needs_update", "代理配置需要更新，请重新设置".to_string()),
        "conflict" => ("unavailable", "暂时无法检查配置，请刷新状态".to_string()),
        "unavailable" => ("unavailable", "未找到应用".to_string()),
        "managed" | "external" => {
            if !proxy_running {
                return (
                    "service_stopped",
                    "代理模式已配置，请先启动本地代理".to_string(),
                );
            }
            if !client_running {
                return ("not_running", "代理配置正常，启动应用后生效".to_string());
            }
            let endpoints = match running_language_server_endpoints(app_path) {
                Ok(endpoints) => endpoints,
                Err(_) => return ("checking", "正在检查配置…".to_string()),
            };
            running_language_server_configuration_status(&endpoints, endpoint)
        }
        _ => ("unavailable", "暂时无法检查配置，请刷新状态".to_string()),
    }
}

pub fn running_language_server_endpoints(app_path: &Path) -> Result<Vec<Option<String>>, String> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "command="])
        .output()
        .map_err(|error| format!("无法检查 Language Server 进程：{error}"))?;
    if !output.status.success() {
        return Err(format!("检查 Language Server 进程失败：{}", output.status));
    }

    let app_marker = app_path.display().to_string();
    let mut endpoints = Vec::new();
    let command_lines = String::from_utf8_lossy(&output.stdout);
    for command_line in command_lines.lines() {
        if !command_line.contains(&app_marker) || !command_line.contains("language_server") {
            continue;
        }
        endpoints.push(command_argument(command_line, "--cloud_code_endpoint"));
    }
    Ok(endpoints)
}

pub fn running_language_server_configuration_status(
    endpoints: &[Option<String>],
    endpoint: &str,
) -> (&'static str, String) {
    if endpoints.is_empty() {
        return ("checking", "正在检查配置…".to_string());
    }
    if endpoints
        .iter()
        .any(|value| value.as_deref() != Some(endpoint))
    {
        ("needs_update", "代理配置需要更新，请重新设置".to_string())
    } else {
        ("matched", "代理配置正常".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_language_server_configuration_requires_all_endpoints_to_match() {
        let endpoint = "http://127.0.0.1:57134";
        assert_eq!(
            running_language_server_configuration_status(&[], endpoint),
            ("checking", "正在检查配置…".to_string())
        );
        assert_eq!(
            running_language_server_configuration_status(&[Some(endpoint.to_string())], endpoint),
            ("matched", "代理配置正常".to_string())
        );
        assert_eq!(
            running_language_server_configuration_status(
                &[Some(endpoint.to_string()), Some(endpoint.to_string())],
                endpoint,
            ),
            ("matched", "代理配置正常".to_string())
        );
        assert_eq!(
            running_language_server_configuration_status(
                &[Some("http://127.0.0.1:56066".to_string())],
                endpoint,
            ),
            ("needs_update", "代理配置需要更新，请重新设置".to_string())
        );
        assert_eq!(
            running_language_server_configuration_status(&[None], endpoint),
            ("needs_update", "代理配置需要更新，请重新设置".to_string())
        );
    }
}
