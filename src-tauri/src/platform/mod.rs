use agy_byok::domain::CustomHostPaths;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as current;
#[cfg(target_os = "windows")]
use windows as current;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod current {
    use super::{AppPaths, HostPaths, IdePaths};
    use std::path::Path;

    pub(super) fn host_paths() -> HostPaths {
        HostPaths::default()
    }

    pub(super) fn validate_custom_app_path(_path: &Path) -> Result<AppPaths, String> {
        Err("当前平台不支持设置自定义路径".to_string())
    }

    pub(super) fn validate_custom_ide_path(_path: &Path) -> Result<IdePaths, String> {
        Err("当前平台不支持设置自定义路径".to_string())
    }

    pub(super) fn open_system_path(_target: &Path) -> Result<(), String> {
        Err("当前平台不支持打开系统路径".to_string())
    }

    pub(super) fn open_external_url(_url: &str) -> Result<(), String> {
        Err("当前平台不支持打开外部链接".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub installation: PathBuf,
    pub executable: PathBuf,
    pub language_server: PathBuf,
    pub is_custom: bool,
}

#[derive(Debug, Clone)]
pub struct IdePaths {
    pub installation: PathBuf,
    pub executable: PathBuf,
    pub settings: Option<PathBuf>,
    pub is_custom: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HostPaths {
    pub app: Option<AppPaths>,
    pub ide: Option<IdePaths>,
}

impl HostPaths {
    pub fn current() -> Self {
        current::host_paths()
    }

    pub fn resolve(custom: &CustomHostPaths) -> Self {
        let mut paths = current::host_paths();
        if let Some(custom_app) = &custom.app {
            if let Ok(app_paths) = current::validate_custom_app_path(custom_app) {
                paths.app = Some(app_paths);
            }
        }
        if let Some(custom_ide) = &custom.ide {
            if let Ok(ide_paths) = current::validate_custom_ide_path(custom_ide) {
                paths.ide = Some(ide_paths);
            }
        }
        paths
    }
}

pub fn validate_custom_app_path(path: &Path) -> Result<AppPaths, String> {
    current::validate_custom_app_path(path)
}

pub fn validate_custom_ide_path(path: &Path) -> Result<IdePaths, String> {
    current::validate_custom_ide_path(path)
}

pub fn open_system_path(target: &std::path::Path) -> Result<(), String> {
    current::open_system_path(target)
}

pub fn open_external_url(url: &str) -> Result<(), String> {
    current::open_external_url(url)
}

fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
