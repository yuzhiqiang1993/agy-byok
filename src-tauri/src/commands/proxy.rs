use crate::state::{DesktopState, ProxyStatus, status_from_handle};
use agy_byok::proxy::{HttpServerOptions, LoopbackHttpServer, ProxyServer};
use agy_byok::storage::AppConfig;
use std::sync::Arc;
use tauri::State;

const OFFICIAL_CLOUD_CODE_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";

#[tauri::command]
pub(crate) fn get_config(state: State<'_, DesktopState>) -> AppConfig {
    state.config_store.get_config()
}

#[tauri::command]
pub(crate) fn save_config(mut config: AppConfig, state: State<'_, DesktopState>) -> Result<AppConfig, String> {
    // 代理端口由桌面运行时管理，必须与前端配置替换在同一写锁内合并。
    state.config_store.update_config_with(move |current| {
        config.proxy_port = current.proxy_port;
        *current = config;
    })
}

#[tauri::command]
pub(crate) async fn proxy_status(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await;
    Ok(status_from_handle(
        handle.as_ref(),
        state.config_store.get_config().proxy_port,
    ))
}

#[tauri::command]
pub(crate) async fn start_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let mut handle = state.proxy_handle.lock().await;
    if handle.is_some() {
        return Ok(status_from_handle(
            handle.as_ref(),
            state.config_store.get_config().proxy_port,
        ));
    }

    let preferred_port = state.config_store.get_config().proxy_port;
    let server = Arc::new(ProxyServer::with_activity_log(
        state.config_store.clone(),
        preferred_port,
        state.activity_log.clone(),
    ));
    let options = HttpServerOptions {
        require_auth: false,
        official_cloud_code_endpoint: Some(OFFICIAL_CLOUD_CODE_ENDPOINT.to_string()),
        fallback_to_random_port_on_bind_error: true,
        ..HttpServerOptions::default()
    };
    let started = LoopbackHttpServer::start(server, options)
        .await
        .map_err(|error| error.to_string())?;
    let actual_port = started.local_addr().port();
    if let Err(error) = state
        .config_store
        .update_config_with(|config| config.proxy_port = actual_port)
    {
        let _ = started.shutdown().await;
        return Err(format!("无法保存本地代理端口：{error}"));
    }
    *handle = Some(started);
    Ok(status_from_handle(handle.as_ref(), actual_port))
}

#[tauri::command]
pub(crate) async fn stop_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await.take();
    if let Some(handle) = handle {
        handle.shutdown().await.map_err(|error| error.to_string())?;
    }
    Ok(ProxyStatus {
        state: "stopped",
        address: Some(format!(
            "127.0.0.1:{}",
            state.config_store.get_config().proxy_port
        )),
    })
}
