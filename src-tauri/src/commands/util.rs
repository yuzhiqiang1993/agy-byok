use agy_byok::storage::default_config_path;

#[tauri::command]
pub(crate) async fn open_path(path: String) -> Result<(), String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return open_external_url(path).await;
    }

    let expanded = if let Some(relative_path) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            home.join(relative_path)
        } else {
            std::path::PathBuf::from(&path)
        }
    } else {
        std::path::PathBuf::from(&path)
    };

    let target = if expanded.exists() {
        expanded
    } else if let Some(parent) = expanded.parent() {
        if parent.exists() {
            parent.to_path_buf()
        } else {
            let _ = std::fs::create_dir_all(parent);
            if parent.exists() {
                parent.to_path_buf()
            } else {
                return Err(format!("目录不存在且无法创建: {}", parent.display()));
            }
        }
    } else {
        return Err(format!("路径不存在: {}", path));
    };

    open_system_path(&target)
}

#[tauri::command]
pub(crate) async fn open_config_dir() -> Result<(), String> {
    let config_path = default_config_path()?;
    if let Some(dir) = config_path.parent() {
        let _ = std::fs::create_dir_all(dir);
        open_system_path(dir)
    } else {
        Err("无法找到配置文件所在目录".to_string())
    }
}

#[tauri::command]
pub(crate) async fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("非法 URL 格式".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("无法打开 URL: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("无法打开 URL: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("无法打开 URL: {e}"))?;
    }
    Ok(())
}

fn open_system_path(target: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("无法打开路径: {e}"))?;
    }
    Ok(())
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
