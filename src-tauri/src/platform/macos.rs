use super::{absolute_environment_path, AppPaths, HostPaths, IdePaths};
use std::path::PathBuf;
use std::process::Command;

pub(super) fn host_paths() -> HostPaths {
    let app_root = PathBuf::from("/Applications/Antigravity.app");
    let ide_root = PathBuf::from("/Applications/Antigravity IDE.app");
    HostPaths {
        app: Some(AppPaths {
            executable: app_root.join("Contents/MacOS/Antigravity"),
            language_server: app_root.join("Contents/Resources/bin/language_server"),
            installation: app_root,
        }),
        ide: Some(IdePaths {
            executable: ide_root.join("Contents/MacOS/Electron"),
            settings: user_home_dir().map(|home| {
                home.join("Library/Application Support/Antigravity IDE/User/settings.json")
            }),
            installation: ide_root,
        }),
    }
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
            PathBuf::from("/Applications/Antigravity.app/Contents/MacOS/Antigravity")
        );
        assert_eq!(
            paths.language_server,
            PathBuf::from("/Applications/Antigravity.app/Contents/Resources/bin/language_server")
        );
    }
}
