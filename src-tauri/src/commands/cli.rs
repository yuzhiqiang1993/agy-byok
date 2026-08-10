use crate::commands::error::{report, HOST_MODIFY_FAILED, HOST_STATUS_FAILED};
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
        host_integration::enable_cli_integration(&integration_root, &endpoint)
            .map_err(|error| error.to_string())?;
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
    let result = tauri::async_runtime::spawn_blocking(move || {
        let current = discover_cli_sync(&integration_root, &endpoint, proxy_running)?;
        if current.integration_state == ClientIntegrationState::Official
            && !current.can_disable_integration
        {
            return Ok(current);
        }
        if !current.can_disable_integration {
            return Err("当前 CLI 没有可恢复的代理配置".to_string());
        }
        host_integration::disable_cli_integration(&integration_root, &endpoint)
            .map_err(|error| error.to_string())?;
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}
