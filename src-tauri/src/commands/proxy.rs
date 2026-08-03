use crate::state::{status_from_handle, DesktopState, ProxyStatus};
use agy_byok::proxy::{HttpServerOptions, LoopbackHttpServer, ProxyServer};
use agy_byok::storage::AppConfig;
use std::sync::Arc;
use tauri::State;

const OFFICIAL_CLOUD_CODE_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
const MIN_PROXY_PORT: u16 = 1024;

#[tauri::command]
pub(crate) fn get_config(state: State<'_, DesktopState>) -> AppConfig {
    state.config_store.get_config()
}

#[tauri::command]
pub(crate) fn save_config(
    mut config: AppConfig,
    state: State<'_, DesktopState>,
) -> Result<AppConfig, String> {
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
pub(crate) async fn set_proxy_port(
    port: u16,
    state: State<'_, DesktopState>,
) -> Result<ProxyStatus, String> {
    set_proxy_port_inner(port, &state).await
}

async fn set_proxy_port_inner(port: u16, state: &DesktopState) -> Result<ProxyStatus, String> {
    validate_proxy_port(port)?;
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let mut handle = state.proxy_handle.lock().await;
    let Some(active_port) = handle.as_ref().map(|active| active.local_addr().port()) else {
        state
            .config_store
            .update_config_with(|config| config.proxy_port = port)?;
        return Ok(status_from_handle(None, port));
    };

    if active_port == port {
        state
            .config_store
            .update_config_with(|config| config.proxy_port = port)?;
        return Ok(status_from_handle(handle.as_ref(), port));
    }

    let replacement =
        LoopbackHttpServer::start(new_proxy_server(state, port), proxy_options(false))
            .await
            .map_err(|error| format!("无法在端口 {port} 启动代理服务：{error}"))?;

    if let Err(error) = state
        .config_store
        .update_config_with(|config| config.proxy_port = port)
    {
        let _ = replacement.shutdown().await;
        return Err(format!("无法保存本地代理端口：{error}"));
    }

    let old_handle = handle
        .replace(replacement)
        .expect("active proxy handle must exist after port validation");
    if let Err(error) = old_handle.shutdown().await {
        tracing::warn!(
            error = %error,
            old_port = active_port,
            new_port = port,
            "旧代理监听已请求关闭，但关闭任务返回错误"
        );
    }

    Ok(status_from_handle(handle.as_ref(), port))
}

#[tauri::command]
pub(crate) async fn start_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let mut handle = state.proxy_handle.lock().await;
    if handle.is_some() {
        return Ok(status_from_handle(
            handle.as_ref(),
            state.config_store.get_config().proxy_port,
        ));
    }

    let preferred_port = state.config_store.get_config().proxy_port;
    let started = LoopbackHttpServer::start(
        new_proxy_server(&state, preferred_port),
        proxy_options(true),
    )
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
    let _mutation_guard = state.proxy_host_mutation_lock.lock().await;
    let handle = state.proxy_handle.lock().await.take();
    if let Some(handle) = handle {
        handle.shutdown().await.map_err(|error| error.to_string())?;
    }
    let port = state.config_store.get_config().proxy_port;
    Ok(ProxyStatus {
        state: "stopped",
        address: Some(format!("127.0.0.1:{port}")),
        port,
    })
}

fn validate_proxy_port(port: u16) -> Result<(), String> {
    if port < MIN_PROXY_PORT {
        return Err(format!("代理端口必须位于 {MIN_PROXY_PORT} - 65535 之间"));
    }
    Ok(())
}

fn new_proxy_server(state: &DesktopState, port: u16) -> Arc<ProxyServer> {
    Arc::new(ProxyServer::with_activity_log(
        state.config_store.clone(),
        port,
        state.activity_log.clone(),
    ))
}

fn proxy_options(fallback_to_random_port: bool) -> HttpServerOptions {
    HttpServerOptions {
        require_auth: false,
        official_cloud_code_endpoint: Some(OFFICIAL_CLOUD_CODE_ENDPOINT.to_string()),
        fallback_to_random_port_on_bind_error: fallback_to_random_port,
        ..HttpServerOptions::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agy_byok::proxy::ActivityLog;
    use agy_byok::storage::{AppConfig, ConfigStore};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    fn test_state() -> DesktopState {
        DesktopState {
            config_store: ConfigStore::in_memory(AppConfig::default()),
            ide_settings_path: PathBuf::new(),
            ide_integration_root: PathBuf::new(),
            activity_log: Arc::new(ActivityLog::new()),
            proxy_host_mutation_lock: tokio::sync::Mutex::new(()),
            proxy_handle: tokio::sync::Mutex::new(None),
        }
    }

    async fn free_port() -> u16 {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        listener.local_addr().unwrap().port()
    }

    async fn free_port_except(excluded: u16) -> u16 {
        loop {
            let port = free_port().await;
            if port != excluded {
                return port;
            }
        }
    }

    async fn start_test_proxy(state: &DesktopState, port: u16) {
        let started =
            LoopbackHttpServer::start(new_proxy_server(state, port), proxy_options(false))
                .await
                .unwrap();
        *state.proxy_handle.lock().await = Some(started);
        state
            .config_store
            .update_config_with(|config| config.proxy_port = port)
            .unwrap();
    }

    async fn stop_test_proxy(state: &DesktopState) {
        if let Some(handle) = state.proxy_handle.lock().await.take() {
            handle.shutdown().await.unwrap();
        }
    }

    #[test]
    fn proxy_port_validation_matches_frontend_range() {
        assert!(validate_proxy_port(1024).is_ok());
        assert!(validate_proxy_port(u16::MAX).is_ok());
        assert!(validate_proxy_port(1023).is_err());
    }

    #[tokio::test]
    async fn changing_stopped_proxy_port_persists_and_returns_stopped_status() {
        let state = test_state();
        let port = free_port().await;

        let status = set_proxy_port_inner(port, &state).await.unwrap();

        assert_eq!(status.state, "stopped");
        assert_eq!(status.port, port);
        assert_eq!(state.config_store.get_config().proxy_port, port);
    }

    #[tokio::test]
    async fn occupied_replacement_port_keeps_existing_proxy_and_config() {
        let state = test_state();
        let old_port = free_port().await;
        let replacement_port = free_port_except(old_port).await;
        start_test_proxy(&state, old_port).await;
        let blocker = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, replacement_port))
            .await
            .unwrap();

        let result = set_proxy_port_inner(replacement_port, &state).await;

        assert!(result.is_err());
        assert_eq!(state.config_store.get_config().proxy_port, old_port);
        assert_eq!(
            state
                .proxy_handle
                .lock()
                .await
                .as_ref()
                .unwrap()
                .local_addr()
                .port(),
            old_port
        );
        drop(blocker);
        stop_test_proxy(&state).await;
    }

    #[tokio::test]
    async fn successful_replacement_switches_to_new_proxy_and_config() {
        let state = test_state();
        let old_port = free_port().await;
        let replacement_port = free_port_except(old_port).await;
        start_test_proxy(&state, old_port).await;

        let status = set_proxy_port_inner(replacement_port, &state)
            .await
            .unwrap();

        assert_eq!(status.state, "running");
        assert_eq!(status.port, replacement_port);
        assert_eq!(state.config_store.get_config().proxy_port, replacement_port);
        assert_eq!(
            state
                .proxy_handle
                .lock()
                .await
                .as_ref()
                .unwrap()
                .local_addr()
                .port(),
            replacement_port
        );
        stop_test_proxy(&state).await;
    }
}
