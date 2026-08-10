use super::*;
use agy_byok::domain::AppConfig;
use agy_byok::proxy::ActivityLog;
use agy_byok::storage::ConfigStore;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

fn test_state() -> DesktopState {
    DesktopState {
        config_store: ConfigStore::in_memory(AppConfig::default()),
        host_integration_root: PathBuf::new(),
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
    let started = LoopbackHttpServer::start(new_proxy_server(state, port), proxy_options(false))
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

    assert_eq!(status.state, ProxyRuntimeState::Stopped);
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

    assert_eq!(status.state, ProxyRuntimeState::Running);
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
