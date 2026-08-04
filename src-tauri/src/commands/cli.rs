use crate::host::cli_host::{discover_cli_sync, CliStatus};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_cli(state: State<'_, DesktopState>) -> Result<CliStatus, String> {
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("CLI discovery task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn enable_cli_integration(
    state: State<'_, DesktopState>,
) -> Result<CliStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let current = discover_cli_sync(&integration_root, &endpoint, proxy_running)?;
        if !current.installed {
            return Err("未找到 Antigravity CLI (agy)，不能启用 CLI 代理模式".to_string());
        }
        if current.integration_state == "managed" && current.configuration_state != "needs_update" {
            return Ok(current);
        }
        if !current.can_enable_integration {
            return Err("当前 CLI 状态不允许启用代理模式".to_string());
        }
        host_integration::enable_cli_integration(&integration_root, &endpoint)
            .map_err(|e| e.to_string())?;
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("CLI integration activation task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn disable_cli_integration(
    state: State<'_, DesktopState>,
) -> Result<CliStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let integration_root = state.ide_integration_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let current = discover_cli_sync(&integration_root, &endpoint, proxy_running)?;
        if !current.can_disable_integration {
            return Err("当前 CLI 没有可恢复的代理配置".to_string());
        }
        host_integration::disable_cli_integration(&integration_root, &endpoint)
            .map_err(|e| e.to_string())?;
        discover_cli_sync(&integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("CLI integration deactivation task failed: {error}"))?
}
