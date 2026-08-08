use std::path::PathBuf;

const CONFIG_PATH_ENV: &str = "AGY_BYOK_CONFIG_PATH";

pub fn default_config_path() -> Result<PathBuf, String> {
    if let Some(path) = config_path_override(std::env::var_os(CONFIG_PATH_ENV))? {
        return Ok(path);
    }

    Ok(config_path_for_root(platform_config_root()?))
}

fn config_path_override(value: Option<std::ffi::OsString>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    absolute_path(Some(value))
        .map(Some)
        .ok_or_else(|| format!("{CONFIG_PATH_ENV} must be an absolute path when it is set"))
}

fn config_path_for_root(root: PathBuf) -> PathBuf {
    root.join("AGY BYOK").join("config.v1.json")
}

#[cfg(target_os = "macos")]
fn platform_config_root() -> Result<PathBuf, String> {
    user_directory("HOME").map(|home| home.join("Library/Application Support"))
}

#[cfg(target_os = "windows")]
fn platform_config_root() -> Result<PathBuf, String> {
    windows_config_root(std::env::var_os("APPDATA"), std::env::var_os("USERPROFILE"))
        .ok_or_else(|| "neither APPDATA nor USERPROFILE is set to an absolute path".to_string())
}

#[cfg(target_os = "windows")]
fn windows_config_root(
    app_data: Option<std::ffi::OsString>,
    user_profile: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    absolute_path(app_data)
        .or_else(|| absolute_path(user_profile).map(|home| home.join("AppData/Roaming")))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_config_root() -> Result<PathBuf, String> {
    if let Some(path) = absolute_path(std::env::var_os("XDG_CONFIG_HOME")) {
        return Ok(path);
    }
    user_directory("HOME").map(|home| home.join(".config"))
}

#[cfg(not(target_os = "windows"))]
fn user_directory(variable: &str) -> Result<PathBuf, String> {
    absolute_path(std::env::var_os(variable))
        .ok_or_else(|| format!("{variable} is not set to an absolute path"))
}

fn absolute_path(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_scoped_to_the_application_directory() {
        assert_eq!(
            config_path_for_root(PathBuf::from("config-root")),
            PathBuf::from("config-root")
                .join("AGY BYOK")
                .join("config.v1.json")
        );
    }

    #[test]
    fn absolute_path_rejects_missing_empty_and_relative_values() {
        assert_eq!(absolute_path(None), None);
        assert_eq!(absolute_path(Some("".into())), None);
        assert_eq!(absolute_path(Some("relative".into())), None);
    }

    #[test]
    fn config_path_override_accepts_only_absolute_paths() {
        assert_eq!(config_path_override(None).unwrap(), None);
        assert_eq!(config_path_override(Some("".into())).unwrap(), None);
        assert!(config_path_override(Some("relative.json".into())).is_err());

        let absolute = std::env::current_dir().unwrap().join("config.json");
        assert_eq!(
            config_path_override(Some(absolute.clone().into_os_string())).unwrap(),
            Some(absolute)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_config_root_prefers_app_data_and_falls_back_to_user_profile() {
        let app_data = std::ffi::OsString::from(r"C:\Users\test\AppData\Roaming");
        let user_profile = std::ffi::OsString::from(r"C:\Users\test");

        assert_eq!(
            windows_config_root(Some(app_data.clone()), Some(user_profile.clone())),
            Some(PathBuf::from(app_data))
        );
        assert_eq!(
            windows_config_root(None, Some(user_profile)),
            Some(PathBuf::from(r"C:\Users\test\AppData\Roaming"))
        );
    }
}
