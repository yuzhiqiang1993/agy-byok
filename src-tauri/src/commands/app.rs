use crate::commands::error::{report, HOST_LAUNCH_FAILED, HOST_MODIFY_FAILED, HOST_STATUS_FAILED};
use crate::host::app_host::{
    disable_integration, discover_app_sync, enable_integration, launch_app as launch_host_app,
    restart_app as restart_host_app, stop_app_for_reconfiguration, AppStatus,
};
use crate::host::{ClientConfigurationState, ClientIntegrationState};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_app(state: State<'_, DesktopState>) -> Result<AppStatus, String> {
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let paths = state.host_paths.app.clone();
    let integration_root = state.host_integration_root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        discover_app_sync(paths.as_ref(), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_STATUS_FAILED, error))?;
    result.map_err(|error| report(HOST_STATUS_FAILED, error))
}

#[tauri::command]
pub(crate) async fn enable_app_integration(
    state: State<'_, DesktopState>,
) -> Result<AppStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let paths = state.host_paths.app.clone();
    let integration_root = state.host_integration_root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台暂不支持 Antigravity App 自动接入".to_string())?;
        let current = discover_app_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if current.integration_state == ClientIntegrationState::Managed
            && current.configuration_state != ClientConfigurationState::NeedsUpdate
        {
            return Ok(current);
        }
        if !current.can_enable_integration {
            return Err("当前 App 状态不允许启用代理模式".to_string());
        }
        let should_restart = stop_app_for_reconfiguration(&paths)?;
        if let Err(error) = enable_integration(&paths, &integration_root, &endpoint) {
            if should_restart {
                let _ = launch_host_app(&paths, None);
            }
            return Err(error);
        }
        if should_restart {
            restart_host_app(&paths, Some(&endpoint))
                .map_err(|error| format!("App 代理模式已启用，但自动重启失败：{error}"))?;
        }
        discover_app_sync(Some(&paths), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}

#[tauri::command]
pub(crate) async fn launch_app(state: State<'_, DesktopState>) -> Result<(), String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let paths = state.host_paths.app.clone();
    let integration_root = state.host_integration_root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台无法定位 Antigravity App".to_string())?;
        let current = discover_app_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if !current.can_launch_app {
            return Err("当前 App 状态不允许打开或重启".to_string());
        }
        let launch_endpoint = current
            .integration_state
            .is_ready()
            .then_some(endpoint.as_str());
        if current.app_running {
            stop_app_for_reconfiguration(&paths)?;
            restart_host_app(&paths, launch_endpoint)
        } else {
            launch_host_app(&paths, launch_endpoint)
        }
    })
    .await
    .map_err(|error| report(HOST_LAUNCH_FAILED, error))?;
    result.map_err(|error| report(HOST_LAUNCH_FAILED, error))
}

#[tauri::command]
pub(crate) async fn disable_app_integration(
    state: State<'_, DesktopState>,
) -> Result<AppStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let paths = state.host_paths.app.clone();
    let integration_root = state.host_integration_root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台暂不支持 Antigravity App 自动接入".to_string())?;
        let current = discover_app_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if current.integration_state == ClientIntegrationState::Official
            && !current.can_disable_integration
        {
            return Ok(current);
        }
        if !current.can_disable_integration {
            return Err("当前 App 没有可恢复的代理配置".to_string());
        }
        let should_restart = stop_app_for_reconfiguration(&paths)?;
        if let Err(error) = disable_integration(&paths, &integration_root, &endpoint) {
            if should_restart {
                let _ = launch_host_app(&paths, Some(&endpoint));
            }
            return Err(error);
        }
        if should_restart {
            restart_host_app(&paths, None)
                .map_err(|error| format!("App 已恢复官方模式，但自动重启失败：{error}"))?;
        }
        discover_app_sync(Some(&paths), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}
