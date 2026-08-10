use super::{absolute_environment_path, AppPaths, HostPaths, IdePaths};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const APP_BUNDLE_ID: &str = "com.google.antigravity";
const IDE_BUNDLE_ID: &str = "com.google.antigravity-ide";
const APP_DEFAULT_PATH: &str = "/Applications/Antigravity.app";
const IDE_DEFAULT_PATH: &str = "/Applications/Antigravity IDE.app";

pub(super) fn host_paths() -> HostPaths {
    let app_root = discover_bundle_installation(
        APP_BUNDLE_ID,
        APP_DEFAULT_PATH,
        &[
            "Contents/MacOS/Antigravity",
            "Contents/Resources/bin/language_server",
        ],
    );
    let ide_root = discover_bundle_installation(
        IDE_BUNDLE_ID,
        IDE_DEFAULT_PATH,
        &["Contents/MacOS/Electron"],
    );
    HostPaths {
        app: Some(AppPaths {
            executable: app_root.join("Contents/MacOS/Antigravity"),
            language_server: app_root.join("Contents/Resources/bin/language_server"),
            installation: app_root,
            is_custom: false,
        }),
        ide: Some(IdePaths {
            executable: ide_root.join("Contents/MacOS/Electron"),
            settings: user_home_dir().map(|home| {
                home.join("Library/Application Support/Antigravity IDE/User/settings.json")
            }),
            installation: ide_root,
            is_custom: false,
        }),
    }
}

pub(super) fn validate_custom_app_path(custom_path: &Path) -> Result<AppPaths, String> {
    let mut candidate = custom_path.to_path_buf();
    while !candidate.as_os_str().is_empty()
        && !candidate.extension().map_or(false, |ext| ext == "app")
    {
        if let Some(parent) = candidate.parent() {
            candidate = parent.to_path_buf();
        } else {
            break;
        }
    }
    let installation = if candidate.extension().map_or(false, |ext| ext == "app") {
        candidate
    } else {
        custom_path.to_path_buf()
    };

    let executable = installation.join("Contents/MacOS/Antigravity");
    let language_server = installation.join("Contents/Resources/bin/language_server");

    if !installation.is_dir() {
        return Err(format!("指定路径不存在或不是有效目录：{}", custom_path.display()));
    }
    if !executable.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity 可执行文件：{}",
            executable.display()
        ));
    }
    if !language_server.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity 核心组件 language_server：{}",
            language_server.display()
        ));
    }

    Ok(AppPaths {
        installation,
        executable,
        language_server,
        is_custom: true,
    })
}

pub(super) fn validate_custom_ide_path(custom_path: &Path) -> Result<IdePaths, String> {
    let mut candidate = custom_path.to_path_buf();
    while !candidate.as_os_str().is_empty()
        && !candidate.extension().map_or(false, |ext| ext == "app")
    {
        if let Some(parent) = candidate.parent() {
            candidate = parent.to_path_buf();
        } else {
            break;
        }
    }
    let installation = if candidate.extension().map_or(false, |ext| ext == "app") {
        candidate
    } else {
        custom_path.to_path_buf()
    };

    let executable = installation.join("Contents/MacOS/Electron");
    if !installation.is_dir() {
        return Err(format!("指定路径不存在或不是有效目录：{}", custom_path.display()));
    }
    if !executable.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity IDE 主程序：{}",
            executable.display()
        ));
    }

    let settings = user_home_dir().map(|home| {
        home.join("Library/Application Support/Antigravity IDE/User/settings.json")
    });

    Ok(IdePaths {
        installation,
        executable,
        settings,
        is_custom: true,
    })
}

fn discover_bundle_installation(
    bundle_id: &str,
    default_path: &str,
    required_files: &[&str],
) -> PathBuf {
    let default_path = PathBuf::from(default_path);
    let mut candidates = vec![default_path.clone()];
    if let (Some(home), Some(bundle_name)) = (user_home_dir(), default_path.file_name()) {
        candidates.push(home.join("Applications").join(bundle_name));
    }
    candidates.extend(spotlight_bundle_paths(bundle_id));

    candidates
        .into_iter()
        .find(|candidate| installation_is_complete(candidate, required_files))
        .unwrap_or(default_path)
}

fn spotlight_bundle_paths(bundle_id: &str) -> Vec<PathBuf> {
    // Spotlight 按 bundle 标识定位应用，兼容用户移动到自定义目录后的安装位置。
    let query = format!("kMDItemCFBundleIdentifier == '{bundle_id}'");
    let Ok(output) = Command::new("/usr/bin/mdfind")
        .args(["-0", &query])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_spotlight_output(&output.stdout)
}

fn parse_spotlight_output(output: &[u8]) -> Vec<PathBuf> {
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(OsString::from_vec(path.to_vec())))
        .collect()
}

fn installation_is_complete(root: &Path, required_files: &[&str]) -> bool {
    root.is_dir()
        && required_files
            .iter()
            .all(|relative_path| root.join(relative_path).is_file())
}

fn user_home_dir() -> Option<PathBuf> {
    absolute_environment_path("HOME")
}

pub(super) fn open_system_path(target: &std::path::Path) -> Result<(), String> {
    Command::new("open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开路径：{error}"))
}

pub(super) fn open_external_url(url: &str) -> Result<(), String> {
    Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开链接：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_app_layout_matches_the_vendor_bundle() {
        let paths = host_paths().app.expect("macOS App paths");

        assert_eq!(
            paths.executable,
            paths.installation.join("Contents/MacOS/Antigravity")
        );
        assert_eq!(
            paths.language_server,
            paths
                .installation
                .join("Contents/Resources/bin/language_server")
        );
    }

    #[test]
    fn parses_nul_delimited_spotlight_paths() {
        assert_eq!(
            parse_spotlight_output(
                b"/Applications/Antigravity IDE.app\0/Volumes/Tools/Antigravity\nIDE.app\0"
            ),
            vec![
                PathBuf::from("/Applications/Antigravity IDE.app"),
                PathBuf::from("/Volumes/Tools/Antigravity\nIDE.app"),
            ]
        );
    }
}
