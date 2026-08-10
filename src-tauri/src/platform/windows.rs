use super::{absolute_environment_path, AppPaths, HostPaths, IdePaths};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY,
};

const APP_EXECUTABLE: &str = "Antigravity.exe";
const IDE_EXECUTABLE: &str = "Antigravity IDE.exe";
const INSTALLER_REGISTRY_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{AA73B3E3-C6C8-45C8-B1DC-4AE56C751432}_is1";

pub(super) fn host_paths() -> HostPaths {
    let local_app_data = absolute_environment_path("LOCALAPPDATA").or_else(|| {
        absolute_environment_path("USERPROFILE").map(|home| home.join("AppData/Local"))
    });
    let roaming_app_data = absolute_environment_path("APPDATA").or_else(|| {
        absolute_environment_path("USERPROFILE").map(|home| home.join("AppData/Roaming"))
    });

    let app = discover_installation(
        APP_EXECUTABLE,
        "Antigravity",
        local_app_data.as_deref(),
        &[APP_EXECUTABLE, "resources/bin/language_server.exe"],
    )
    .map(|installation| AppPaths {
        executable: installation.join(APP_EXECUTABLE),
        language_server: installation.join("resources/bin/language_server.exe"),
        installation,
        is_custom: false,
    });
    let ide = discover_installation(
        IDE_EXECUTABLE,
        "Antigravity IDE",
        local_app_data.as_deref(),
        &[IDE_EXECUTABLE],
    )
    .map(|installation| IdePaths {
        executable: installation.join(IDE_EXECUTABLE),
        settings: roaming_app_data.map(|root| root.join("Antigravity IDE/User/settings.json")),
        installation,
        is_custom: false,
    });

    HostPaths { app, ide }
}

pub(super) fn validate_custom_app_path(custom_path: &Path) -> Result<AppPaths, String> {
    let installation = if custom_path.is_file() {
        custom_path
            .parent()
            .ok_or_else(|| "无效的文件路径".to_string())?
            .to_path_buf()
    } else {
        custom_path.to_path_buf()
    };

    let executable = installation.join(APP_EXECUTABLE);
    let language_server = installation.join("resources/bin/language_server.exe");

    if !installation.is_dir() {
        return Err(format!("指定路径不存在或不是有效目录：{}", custom_path.display()));
    }
    if !executable.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity 主程序（{}）：{}",
            APP_EXECUTABLE,
            executable.display()
        ));
    }
    if !language_server.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity 核心组件：{}",
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
    let installation = if custom_path.is_file() {
        custom_path
            .parent()
            .ok_or_else(|| "无效的文件路径".to_string())?
            .to_path_buf()
    } else {
        custom_path.to_path_buf()
    };

    let executable = installation.join(IDE_EXECUTABLE);
    if !installation.is_dir() {
        return Err(format!("指定路径不存在或不是有效目录：{}", custom_path.display()));
    }
    if !executable.is_file() {
        return Err(format!(
            "在所选路径中未找到 Antigravity IDE 主程序（{}）：{}",
            IDE_EXECUTABLE,
            executable.display()
        ));
    }

    let roaming_app_data = absolute_environment_path("APPDATA").or_else(|| {
        absolute_environment_path("USERPROFILE").map(|home| home.join("AppData/Roaming"))
    });
    let settings = roaming_app_data.map(|root| root.join("Antigravity IDE/User/settings.json"));

    Ok(IdePaths {
        installation,
        executable,
        settings,
        is_custom: true,
    })
}

fn discover_installation(
    executable_name: &str,
    directory_name: &str,
    local_app_data: Option<&Path>,
    required_files: &[&str],
) -> Option<PathBuf> {
    let fallback = local_app_data.map(|root| root.join("Programs").join(directory_name));
    let mut candidates = registered_installation_candidates(executable_name);
    if let Some(path) = fallback.as_ref() {
        candidates.push(path.clone());
    }
    for environment_name in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = absolute_environment_path(environment_name) {
            candidates.push(root.join(directory_name));
        }
    }

    candidates
        .into_iter()
        .find(|candidate| installation_is_complete(candidate, required_files))
        .or(fallback)
}

fn registered_installation_candidates(executable_name: &str) -> Vec<PathBuf> {
    let app_path_key =
        format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{executable_name}");
    let mut candidates = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for registry_view in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY] {
            if let Some(executable) = registry_string(root, &app_path_key, None, registry_view) {
                if let Some(parent) = PathBuf::from(executable).parent() {
                    candidates.push(parent.to_path_buf());
                }
            }
            if let Some(installation) = registry_string(
                root,
                INSTALLER_REGISTRY_KEY,
                Some("InstallLocation"),
                registry_view,
            ) {
                candidates.push(PathBuf::from(installation));
            }
        }
    }
    candidates
}

fn registry_string(
    root: HKEY,
    sub_key: &str,
    value_name: Option<&str>,
    registry_view: u32,
) -> Option<OsString> {
    let sub_key = wide_null(sub_key);
    let value_name = value_name.map(wide_null);
    let value_name_ptr = value_name
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | registry_view;
    let mut byte_length = 0_u32;

    let status = unsafe {
        RegGetValueW(
            root,
            sub_key.as_ptr(),
            value_name_ptr,
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut byte_length,
        )
    };
    if status != 0 || byte_length < 2 {
        return None;
    }

    let mut value = vec![0_u16; (byte_length as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            root,
            sub_key.as_ptr(),
            value_name_ptr,
            flags,
            std::ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut byte_length,
        )
    };
    if status != 0 {
        return None;
    }

    value.truncate((byte_length as usize / 2).min(value.len()));
    while value.last() == Some(&0) {
        value.pop();
    }
    if value.len() >= 2
        && value.first() == Some(&u16::from(b'"'))
        && value.last() == Some(&u16::from(b'"'))
    {
        value.remove(0);
        value.pop();
    }
    (!value.is_empty()).then(|| OsString::from_wide(&value))
}

fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn installation_is_complete(root: &Path, required_files: &[&str]) -> bool {
    root.is_dir()
        && required_files
            .iter()
            .all(|relative_path| root.join(relative_path).is_file())
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
    use super::*;

    #[test]
    fn windows_ide_layout_matches_the_vendor_user_installer() {
        let installation =
            PathBuf::from(r"C:\Users\test\AppData\Local").join("Programs/Antigravity IDE");
        assert_eq!(
            installation.join(IDE_EXECUTABLE),
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
            installation.join(APP_EXECUTABLE),
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
