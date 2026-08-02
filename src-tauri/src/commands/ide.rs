use crate::host::ide_host::{
    discover_ide_sync, launch_ide_app, restart_ide_app, stop_ide_for_reconfiguration,
    IdeStatus, ANTIGRAVITY_IDE_PATH,
};
use crate::state::{get_active_proxy_endpoint, is_proxy_running, DesktopState};
use host_integration::{disable_ide_settings, enable_ide_settings};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub(crate) async fn discover_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("IDE discovery task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn enable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    if !proxy_running {
        return Err("请先启动 AGY BYOK 本地代理，再启用模型接入".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
        let current =
            discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)?;
        if matches!(current.integration_state, "managed" | "external")
            && current.configuration_state != "needs_update"
        {
            return Ok(current);
        }
        if !current.compatible {
            return Err(current.message);
        }
        if !current.can_enable_integration {
            return Err(current.integration_message);
        }

        let restart_ide = stop_ide_for_reconfiguration(app_path, "Antigravity IDE")?;
        if let Err(error) = enable_ide_settings(&settings_path, &integration_root, &endpoint) {
            if restart_ide {
                let _ = launch_ide_app();
            }
            return Err(error.to_string());
        }
        if restart_ide {
            restart_ide_app(app_path, "Antigravity IDE")
                .map_err(|error| format!("IDE 接入已启用，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("IDE integration activation task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn launch_ide(state: State<'_, DesktopState>) -> Result<(), String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let current =
            discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)?;
        if !current.compatible {
            return Err(current.message);
        }
        if !current.can_launch_ide {
            return Err(
                "Antigravity IDE 当前不可启动，请检查安装状态或退出正在运行的 IDE".to_string(),
            );
        }
        if current.integration_state != "disabled" && !proxy_running {
            return Err("当前 IDE 已接入本地代理，请先启动 AGY BYOK 本地代理".to_string());
        }
        launch_ide_app()
    })
    .await
    .map_err(|error| format!("IDE launch task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn disable_ide_integration(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let settings_path = state.ide_settings_path.clone();
    let integration_root = state.ide_integration_root.clone();
    let endpoint = get_active_proxy_endpoint(&state).await;
    let proxy_running = is_proxy_running(&state).await;
    tauri::async_runtime::spawn_blocking(move || {
        let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
        let current =
            discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)?;
        if current.integration_state == "official" && !current.can_disable_integration {
            return Ok(current);
        }
        if !current.can_disable_integration {
            return Err(current.integration_message);
        }

        let restart_ide = if current.ide_running {
            stop_ide_for_reconfiguration(app_path, "Antigravity IDE")?
        } else {
            false
        };
        if let Err(error) = disable_ide_settings(&settings_path, &integration_root, &endpoint) {
            if restart_ide {
                let _ = launch_ide_app();
            }
            return Err(error.to_string());
        }
        if restart_ide {
            restart_ide_app(app_path, "Antigravity IDE")
                .map_err(|error| format!("IDE 接入已停用，但自动重启失败：{error}"))?;
        }
        discover_ide_sync(&settings_path, &integration_root, &endpoint, proxy_running)
    })
    .await
    .map_err(|error| format!("IDE integration deactivation task failed: {error}"))?
}
