#[tauri::command]
pub(crate) async fn open_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    let target = if p.exists() {
        p
    } else if let Some(parent) = p.parent() {
        if parent.exists() {
            parent
        } else {
            return Err(format!("文件及目录均不存在: {}", path));
        }
    } else {
        return Err(format!("路径不存在: {}", path));
    };

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
