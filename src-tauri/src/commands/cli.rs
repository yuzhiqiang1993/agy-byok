use crate::commands::error::{report, HOST_MODIFY_FAILED, HOST_STATUS_FAILED};
use crate::host::app_host::{
    launch_app as launch_host_app, restart_app as restart_host_app, stop_app_for_reconfiguration,
};
use crate::host::cli_host::{discover_cli_sync, CliStatus};
use crate::host::{ClientConfigurationState, ClientIntegrationState};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_cli(state: State<'_, DesktopState>) -> Result<CliStatus, String> {
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.host_integration_root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_STATUS_FAILED, error))?;
    result.map_err(|error| report(HOST_STATUS_FAILED, error))
}

#[tauri::command]
pub(crate) async fn enable_cli_integration(
    state: State<'_, DesktopState>,
) -> Result<CliStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.host_integration_root.clone();
    let app_paths = state.host_paths.app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let current = discover_cli_sync(&integration_root, &endpoint, proxy_running)?;
        if !current.installed {
            return Err("未找到 Antigravity CLI (agy)，不能启用 CLI 代理模式".to_string());
        }
        if current.integration_state == ClientIntegrationState::Managed
            && current.configuration_state != ClientConfigurationState::NeedsUpdate
        {
            return Ok(current);
        }
        if !current.can_enable_integration {
            return Err("当前 CLI 状态不允许启用代理模式".to_string());
        }
        let app_was_running = stop_installed_app(app_paths.as_ref())?;
        if let Err(error) = host_integration::enable_cli_integration(&integration_root, &endpoint) {
            if app_was_running {
                if let Some(paths) = app_paths.as_ref() {
                    let _ = launch_host_app(paths, None);
                }
            }
            return Err(error.to_string());
        }
        if app_was_running {
            let paths = app_paths
                .as_ref()
                .ok_or_else(|| "同步重启 App 时缺少安装路径".to_string())?;
            restart_host_app(paths, Some(&endpoint))
                .map_err(|error| format!("CLI 代理模式已启用，但同步重启 App 失败：{error}"))?;
        }
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}

#[tauri::command]
pub(crate) async fn disable_cli_integration(
    state: State<'_, DesktopState>,
) -> Result<CliStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.host_integration_root.clone();
    let app_paths = state.host_paths.app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let current = discover_cli_sync(&integration_root, &endpoint, proxy_running)?;
        if !current.can_disable_integration {
            return Err("当前 CLI 没有可恢复的代理配置".to_string());
        }
        let app_was_running = stop_installed_app(app_paths.as_ref())?;
        if let Err(error) = host_integration::disable_cli_integration(&integration_root, &endpoint)
        {
            if app_was_running {
                if let Some(paths) = app_paths.as_ref() {
                    let _ = launch_host_app(paths, Some(&endpoint));
                }
            }
            return Err(error.to_string());
        }
        if app_was_running {
            let paths = app_paths
                .as_ref()
                .ok_or_else(|| "同步重启 App 时缺少安装路径".to_string())?;
            restart_host_app(paths, None)
                .map_err(|error| format!("CLI 代理模式已停用，但同步重启 App 失败：{error}"))?;
        }
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}

fn stop_installed_app(paths: Option<&crate::platform::AppPaths>) -> Result<bool, String> {
    match paths.filter(|paths| paths.installation.is_dir() && paths.executable.is_file()) {
        Some(paths) => stop_app_for_reconfiguration(paths),
        None => Ok(false),
    }
}
