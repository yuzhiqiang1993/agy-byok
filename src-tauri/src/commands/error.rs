use std::fmt::Display;

pub(super) const HOST_STATUS_FAILED: &str = "host_status_failed";
pub(super) const HOST_MODIFY_FAILED: &str = "host_modify_failed";
pub(super) const HOST_LAUNCH_FAILED: &str = "host_launch_failed";
pub(super) const PROVIDER_CATALOG_FAILED: &str = "provider_catalog_failed";
pub(super) const OFFICIAL_MODELS_HOST_NOT_INSTALLED: &str = "official_models_host_not_installed";
pub(super) const OFFICIAL_MODELS_HOST_NOT_RUNNING: &str = "official_models_host_not_running";
pub(super) const OFFICIAL_MODELS_PROXY_REQUIRED: &str = "official_models_proxy_required";
pub(super) const OFFICIAL_MODELS_FETCH_FAILED: &str = "official_models_fetch_failed";
pub(super) const CONFIG_SAVE_FAILED: &str = "config_save_failed";
pub(super) const PROXY_RECONFIGURE_FAILED: &str = "proxy_reconfigure_failed";
pub(super) const PROXY_START_FAILED: &str = "proxy_start_failed";
pub(super) const PROXY_STOP_FAILED: &str = "proxy_stop_failed";
pub(super) const PATH_OPEN_FAILED: &str = "path_open_failed";
pub(super) const CONFIG_PATH_FAILED: &str = "config_path_failed";
pub(super) const CONFIG_DIR_OPEN_FAILED: &str = "config_dir_open_failed";
pub(super) const EXTERNAL_URL_INVALID: &str = "external_url_invalid";
pub(super) const EXTERNAL_URL_OPEN_FAILED: &str = "external_url_open_failed";
pub(super) const NATIVE_LOCALE_UPDATE_FAILED: &str = "native_locale_update_failed";

pub(super) fn report(code: &'static str, error: impl Display) -> String {
    tracing::error!(error_code = code, error = %error, "桌面命令执行失败");
    code.to_string()
}
