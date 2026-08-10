mod startup_error;

pub use startup_error::StartupError;

use crate::platform::HostPaths;
use agy_byok::proxy::{ActivityLog, HttpServerHandle};
use agy_byok::storage::{default_config_path, ConfigStore};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub state: ProxyRuntimeState,
    pub address: Option<String>,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRuntimeState {
    Running,
    Stopped,
}

pub struct DesktopState {
    pub config_store: ConfigStore,
    pub host_integration_root: PathBuf,
    pub activity_log: Arc<ActivityLog>,
    // Mutating commands acquire this before proxy_handle to keep host writes and proxy changes ordered.
    pub proxy_host_mutation_lock: Mutex<()>,
    pub proxy_handle: Mutex<Option<HttpServerHandle>>,
}

impl DesktopState {
    pub fn current_host_paths(&self) -> HostPaths {
        let custom = &self.config_store.get_config().custom_host_paths;
        HostPaths::resolve(custom)
    }
}

pub struct ProxyRuntimeSnapshot {
    pub endpoint: String,
    pub running: bool,
}

pub fn local_proxy_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn status_from_handle(handle: Option<&HttpServerHandle>, configured_port: u16) -> ProxyStatus {
    match handle {
        Some(handle) => ProxyStatus {
            state: ProxyRuntimeState::Running,
            address: Some(handle.local_addr().to_string()),
            port: handle.local_addr().port(),
        },
        None => ProxyStatus {
            state: ProxyRuntimeState::Stopped,
            address: Some(format!("127.0.0.1:{configured_port}")),
            port: configured_port,
        },
    }
}

pub async fn proxy_runtime_snapshot(state: &DesktopState) -> ProxyRuntimeSnapshot {
    let handle = state.proxy_handle.lock().await;
    let (port, running) = match handle.as_ref() {
        Some(handle) => (handle.local_addr().port(), true),
        None => (state.config_store.get_config().proxy_port, false),
    };
    ProxyRuntimeSnapshot {
        endpoint: local_proxy_endpoint(port),
        running,
    }
}

pub fn create_state() -> Result<DesktopState, StartupError> {
    let config_path = default_config_path().map_err(StartupError::ConfigPath)?;
    let config_exists = config_path.exists();
    let app_support_root = config_path
        .parent()
        .ok_or_else(|| StartupError::MissingConfigParent(config_path.clone()))?;
    let host_integration_root = app_support_root.join("host-integration");
    let config_error = |source| StartupError::Config {
        path: config_path.clone(),
        source,
    };
    let config_store = ConfigStore::load_from_file(&config_path).map_err(&config_error)?;
    if !config_exists {
        config_store
            .update_config(config_store.get_config())
            .map_err(config_error)?;
    }

    Ok(DesktopState {
        config_store,
        host_integration_root,
        activity_log: Arc::new(ActivityLog::new()),
        proxy_host_mutation_lock: Mutex::new(()),
        proxy_handle: Mutex::new(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_runtime_state_codes_match_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(ProxyRuntimeState::Running).unwrap(),
            "running"
        );
        assert_eq!(
            serde_json::to_value(ProxyRuntimeState::Stopped).unwrap(),
            "stopped"
        );
    }
}
