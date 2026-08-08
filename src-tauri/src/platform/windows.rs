use super::{absolute_environment_path, HostPaths, IdePaths};
use std::process::Command;

pub(super) fn host_paths() -> HostPaths {
    let local_app_data = absolute_environment_path("LOCALAPPDATA").or_else(|| {
        absolute_environment_path("USERPROFILE").map(|home| home.join("AppData/Local"))
    });
    let roaming_app_data = absolute_environment_path("APPDATA").or_else(|| {
        absolute_environment_path("USERPROFILE").map(|home| home.join("AppData/Roaming"))
    });

    let app = local_app_data.as_ref().map(|root| {
        let installation = root.join("Programs/Antigravity");
        super::AppPaths {
            executable: installation.join("Antigravity.exe"),
            language_server: installation.join("resources/bin/language_server.exe"),
            installation,
        }
    });
    let ide = local_app_data.map(|root| {
        let installation = root.join("Programs/Antigravity IDE");
        IdePaths {
            executable: installation.join("Antigravity IDE.exe"),
            settings: roaming_app_data.map(|root| root.join("Antigravity IDE/User/settings.json")),
            installation,
        }
    });

    HostPaths { app, ide }
}

pub(super) fn open_system_path(target: &std::path::Path) -> Result<(), String> {
    Command::new("explorer")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开路径：{error}"))
}

pub(super) fn open_external_url(url: &str) -> Result<(), String> {
    Command::new("explorer")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开链接：{error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn windows_ide_layout_matches_the_vendor_user_installer() {
        let installation =
            PathBuf::from(r"C:\Users\test\AppData\Local").join("Programs/Antigravity IDE");
        assert_eq!(
            installation.join("Antigravity IDE.exe"),
            PathBuf::from(
                r"C:\Users\test\AppData\Local\Programs\Antigravity IDE\Antigravity IDE.exe"
            )
        );
    }

    #[test]
    fn windows_app_layout_matches_the_vendor_user_installer() {
        let installation =
            PathBuf::from(r"C:\Users\test\AppData\Local").join("Programs/Antigravity");
        assert_eq!(
            installation.join("Antigravity.exe"),
            PathBuf::from(r"C:\Users\test\AppData\Local\Programs\Antigravity\Antigravity.exe")
        );
        assert_eq!(
            installation.join("resources/bin/language_server.exe"),
            PathBuf::from(
                r"C:\Users\test\AppData\Local\Programs\Antigravity\resources\bin\language_server.exe"
            )
        );
    }
}
