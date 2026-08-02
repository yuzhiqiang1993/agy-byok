use agy_byok::proxy::{ActivityLog, HttpServerHandle};
use agy_byok::storage::{default_config_path, ConfigStore};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub state: &'static str,
    pub address: Option<String>,
}

pub struct DesktopState {
    pub config_store: ConfigStore,
    pub ide_settings_path: PathBuf,
    pub ide_integration_root: PathBuf,
    pub activity_log: Arc<ActivityLog>,
    pub proxy_handle: Mutex<Option<HttpServerHandle>>,
}

pub fn local_proxy_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn status_from_handle(handle: Option<&HttpServerHandle>, configured_port: u16) -> ProxyStatus {
    match handle {
        Some(handle) => ProxyStatus {
            state: "running",
            address: Some(handle.local_addr().to_string()),
        },
        None => ProxyStatus {
            state: "stopped",
            address: Some(format!("127.0.0.1:{configured_port}")),
        },
    }
}

pub async fn is_proxy_running(state: &DesktopState) -> bool {
    state.proxy_handle.lock().await.is_some()
}

pub async fn get_active_proxy_endpoint(state: &DesktopState) -> String {
    let handle = state.proxy_handle.lock().await;
    let port = handle
        .as_ref()
        .map(|h| h.local_addr().port())
        .unwrap_or_else(|| state.config_store.get_config().proxy_port);
    local_proxy_endpoint(port)
}

pub fn create_state() -> Result<DesktopState, String> {
    let config_path = default_config_path()?;
    let config_exists = config_path.exists();
    let app_support_root = config_path
        .parent()
        .ok_or_else(|| "AGY BYOK 配置路径缺少父目录".to_string())?;
    let ide_integration_root = app_support_root.join("ide-integration");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位用户主目录，不能配置 Antigravity IDE".to_string())?;
    if !home.is_absolute() {
        return Err("用户主目录不是绝对路径，不能配置 Antigravity IDE".to_string());
    }
    let ide_settings_path =
        home.join("Library/Application Support/Antigravity IDE/User/settings.json");
    let config_store = ConfigStore::load_from_file(&config_path)?;
    if !config_exists {
        config_store.update_config(config_store.get_config())?;
    }

    Ok(DesktopState {
        config_store,
        ide_settings_path,
        ide_integration_root,
        activity_log: Arc::new(ActivityLog::new()),
        proxy_handle: Mutex::new(None),
    })
}
