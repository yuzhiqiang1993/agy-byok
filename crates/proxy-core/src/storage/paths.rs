use std::path::{Path, PathBuf};

const CONFIG_PATH_ENV: &str = "AGY_BYOK_CONFIG_PATH";

pub fn default_config_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(CONFIG_PATH_ENV) {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| "HOME is not set; cannot resolve the AGY BYOK config path".to_string())?;
    Ok(config_path_for_home(Path::new(&home)))
}

fn config_path_for_home(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("AGY BYOK")
            .join("config.v1.json")
    }

    #[cfg(not(target_os = "macos"))]
    {
        home.join(".config").join("agy-byok").join("config.v1.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_uses_platform_application_data_directory() {
        let path = config_path_for_home(Path::new("/Users/test"));

        #[cfg(target_os = "macos")]
        assert_eq!(
            path,
            PathBuf::from("/Users/test/Library/Application Support/AGY BYOK/config.v1.json")
        );

        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            path,
            PathBuf::from("/Users/test/.config/agy-byok/config.v1.json")
        );
    }
}
