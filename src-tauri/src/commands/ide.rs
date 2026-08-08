use crate::commands::error::{report, HOST_LAUNCH_FAILED, HOST_MODIFY_FAILED, HOST_STATUS_FAILED};
use crate::host::ide_host::{
    discover_ide_sync, launch_ide as launch_host_ide, restart_ide as restart_host_ide,
    stop_ide_for_reconfiguration, IdeStatus,
};
use crate::host::{ClientConfigurationState, ClientIntegrationState};
use crate::state::{proxy_runtime_snapshot, DesktopState};
use host_integration::{disable_ide_settings, enable_ide_settings};
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let paths = state.host_paths.ide.clone();
    let integration_root = state.host_integration_root.clone();
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let result = tauri::async_runtime::spawn_blocking(move || {
        discover_ide_sync(paths.as_ref(), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_STATUS_FAILED, error))?;
    result.map_err(|error| report(HOST_STATUS_FAILED, error))
}

#[tauri::command]
pub(crate) async fn enable_ide_integration(
    state: State<'_, DesktopState>,
) -> Result<IdeStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let paths = state.host_paths.ide.clone();
    let integration_root = state.host_integration_root.clone();
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台无法定位 Antigravity IDE".to_string())?;
        let settings_path = paths
            .settings
            .as_deref()
            .ok_or_else(|| "无法定位 Antigravity IDE 用户设置文件".to_string())?;
        let current = discover_ide_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if current.integration_state.is_ready()
            && current.configuration_state != ClientConfigurationState::NeedsUpdate
        {
            return Ok(current);
        }
        if !current.compatible {
            return Err("Antigravity IDE 当前不可用".to_string());
        }
        if !current.can_enable_integration {
            return Err("当前 IDE 状态不允许启用代理模式".to_string());
        }

        let should_restart = stop_ide_for_reconfiguration(&paths)?;
        if let Err(error) = enable_ide_settings(settings_path, &integration_root, &endpoint) {
            if should_restart {
                let _ = launch_host_ide(&paths);
            }
            return Err(error.to_string());
        }
        if should_restart {
            restart_host_ide(&paths)
                .map_err(|error| format!("IDE 代理模式已启用，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(Some(&paths), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}

#[tauri::command]
pub(crate) async fn launch_ide(state: State<'_, DesktopState>) -> Result<(), String> {
    let paths = state.host_paths.ide.clone();
    let integration_root = state.host_integration_root.clone();
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台无法定位 Antigravity IDE".to_string())?;
        let current = discover_ide_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if !current.compatible {
            return Err("Antigravity IDE 当前不可用".to_string());
        }
        if !current.can_launch_ide {
            return Err(
                "Antigravity IDE 当前不可启动，请检查安装状态或退出正在运行的 IDE".to_string(),
            );
        }
        if current.integration_state.is_ready() && !proxy_running {
            return Err("当前 IDE 已启用代理模式，请先启动 AGY BYOK 本地代理".to_string());
        }
        launch_host_ide(&paths)
    })
    .await
    .map_err(|error| report(HOST_LAUNCH_FAILED, error))?;
    result.map_err(|error| report(HOST_LAUNCH_FAILED, error))
}

#[tauri::command]
pub(crate) async fn disable_ide_integration(
    state: State<'_, DesktopState>,
) -> Result<IdeStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let paths = state.host_paths.ide.clone();
    let integration_root = state.host_integration_root.clone();
    let snapshot = proxy_runtime_snapshot(&state).await;
    let endpoint = snapshot.endpoint;
    let proxy_running = snapshot.running;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths = paths.ok_or_else(|| "当前平台无法定位 Antigravity IDE".to_string())?;
        let settings_path = paths
            .settings
            .as_deref()
            .ok_or_else(|| "无法定位 Antigravity IDE 用户设置文件".to_string())?;
        let current = discover_ide_sync(Some(&paths), &integration_root, &endpoint, proxy_running)?;
        if current.integration_state == ClientIntegrationState::Official
            && !current.can_disable_integration
        {
            return Ok(current);
        }
        if !current.can_disable_integration {
            return Err("当前 IDE 没有可恢复的代理配置".to_string());
        }

        let should_restart = stop_ide_for_reconfiguration(&paths)?;
        if let Err(error) = disable_ide_settings(settings_path, &integration_root, &endpoint) {
            if should_restart {
                let _ = launch_host_ide(&paths);
            }
            return Err(error.to_string());
        }
        if should_restart {
            restart_host_ide(&paths)
                .map_err(|error| format!("IDE 已恢复官方模式，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(Some(&paths), &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| report(HOST_MODIFY_FAILED, error))?;
    result.map_err(|error| report(HOST_MODIFY_FAILED, error))
}
