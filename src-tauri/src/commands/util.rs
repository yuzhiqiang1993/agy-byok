use agy_byok::storage::default_config_path;
use tauri::AppHandle;

use crate::commands::error::{
    report, CONFIG_DIR_OPEN_FAILED, CONFIG_PATH_FAILED, EXTERNAL_URL_INVALID,
    EXTERNAL_URL_OPEN_FAILED, NATIVE_LOCALE_UPDATE_FAILED, PATH_OPEN_FAILED,
};
use crate::native_ui::NativeLocale;
use crate::platform;

#[tauri::command]
pub(crate) async fn open_path(path: String) -> Result<(), String> {
    open_path_inner(path).map_err(|error| report(PATH_OPEN_FAILED, error))
}

fn open_path_inner(path: String) -> Result<(), String> {
    let expanded = std::path::PathBuf::from(&path);

    let target = if expanded.exists() {
        expanded
    } else if let Some(parent) = expanded.parent() {
        if parent.exists() {
            parent.to_path_buf()
        } else {
            return Err(format!("路径及其父目录均不存在: {}", expanded.display()));
        }
    } else {
        return Err(format!("路径不存在: {}", path));
    };

    platform::open_system_path(&target)
}

#[tauri::command]
pub(crate) async fn open_config_dir() -> Result<(), String> {
    open_config_dir_inner().map_err(|error| report(CONFIG_DIR_OPEN_FAILED, error))
}

fn open_config_dir_inner() -> Result<(), String> {
    let config_path = default_config_path()?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("无法创建配置目录 {}：{error}", dir.display()))?;
        platform::open_system_path(dir)
    } else {
        Err("无法找到配置文件所在目录".to_string())
    }
}

#[tauri::command]
pub(crate) async fn get_config_path() -> Result<String, String> {
    default_config_path()
        .map(|path| path.display().to_string())
        .map_err(|error| report(CONFIG_PATH_FAILED, error))
}

#[tauri::command]
pub(crate) fn set_native_locale(app: AppHandle, locale: String) -> Result<(), String> {
    let locale = NativeLocale::from_tag(&locale);
    crate::set_tray_locale(&app, locale)
        .map_err(|error| report(NATIVE_LOCALE_UPDATE_FAILED, error))?;
    locale
        .persist()
        .map_err(|error| report(NATIVE_LOCALE_UPDATE_FAILED, error))
}

#[tauri::command]
pub(crate) async fn open_external_url(url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|_| EXTERNAL_URL_INVALID.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(EXTERNAL_URL_INVALID.to_string());
    }
    platform::open_external_url(parsed.as_str())
        .map_err(|error| report(EXTERNAL_URL_OPEN_FAILED, error))
}
