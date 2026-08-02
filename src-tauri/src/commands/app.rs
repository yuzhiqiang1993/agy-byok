use crate::host::app_host::{
    discover_app_sync, launch_app_app, restart_app_app, stop_app_for_reconfiguration, AppStatus,
    ANTIGRAVITY_APP_PATH,
};
use crate::state::{get_active_proxy_endpoint, is_proxy_running, DesktopState};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_app(state: State<'_, DesktopState>) -> Result<AppStatus, String> {
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    discover_app_sync(&endpoint, proxy_running)
}

#[tauri::command]
pub(crate) async fn enable_app_integration(
    state: State<'_, DesktopState>,
) -> Result<AppStatus, String> {
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    if !proxy_running {
        return Err("请先启动 AGY BYOK 本地代理，再启用 App 代理模式".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_APP_PATH);
        let current = discover_app_sync(&endpoint, proxy_running)?;
        if current.integration_state == "managed" && current.configuration_state != "needs_update" {
            return Ok(current);
        }
        if !current.can_enable_integration {
            return Err(current.integration_message);
        }
        let restart_app = if current.app_running {
            stop_app_for_reconfiguration(app_path, "Antigravity")?
        } else {
            false
        };
        if let Err(error) = host_integration::enable_app_integration(app_path, &endpoint) {
            if restart_app {
                let _ = launch_app_app(app_path);
            }
            return Err(error.to_string());
        }
        if restart_app {
            restart_app_app(app_path)
                .map_err(|error| format!("App 代理模式已启用，但自动重启失败：{error}"))?;
        }
        discover_app_sync(&endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("App integration activation task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn launch_app(state: State<'_, DesktopState>) -> Result<(), String> {
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_APP_PATH);
        let current = discover_app_sync(&endpoint, proxy_running)?;
        if !current.can_launch_app {
            return Err(current.integration_message);
        }
        launch_app_app(app_path)
    })
    .await
    .map_err(|error| format!("App launch task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn disable_app_integration(
    state: State<'_, DesktopState>,
) -> Result<AppStatus, String> {
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_APP_PATH);
        let current = discover_app_sync(&endpoint, proxy_running)?;
        let restart_app = if current.app_running {
            stop_app_for_reconfiguration(app_path, "Antigravity")?
        } else {
            false
        };
        if let Err(error) = host_integration::disable_app_integration(app_path, &endpoint) {
            if restart_app {
                let _ = launch_app_app(app_path);
            }
            return Err(error.to_string());
        }
        if restart_app {
            restart_app_app(app_path)
                .map_err(|error| format!("App 已恢复官方模式，但自动重启失败：{error}"))?;
        }
        discover_app_sync(&endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("App integration deactivation task failed: {error}"))?
}
