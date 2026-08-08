use crate::error::HostIntegrationError;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA};
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ,
    RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

const USER_ENVIRONMENT_SUBKEY: &str = "Environment";
const CLOUD_CODE_URL: &str = "CLOUD_CODE_URL";
const MAX_USER_ENVIRONMENT_VALUE_BYTES: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum RegistryStringKind {
    String,
    ExpandableString,
}

impl RegistryStringKind {
    fn from_registry_type(value_type: u32) -> Result<Self, HostIntegrationError> {
        match value_type {
            REG_SZ => Ok(Self::String),
            REG_EXPAND_SZ => Ok(Self::ExpandableString),
            _ => Err(HostIntegrationError::InvalidIntegration(
                "Windows 用户级 CLOUD_CODE_URL 不是字符串类型，已拒绝覆盖".to_string(),
            )),
        }
    }

    fn registry_type(self) -> u32 {
        match self {
            Self::String => REG_SZ,
            Self::ExpandableString => REG_EXPAND_SZ,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistryStringValue {
    pub(super) value: String,
    pub(super) kind: RegistryStringKind,
}

pub(super) fn read_user_environment_value(
) -> Result<Option<RegistryStringValue>, HostIntegrationError> {
    let subkey = wide(USER_ENVIRONMENT_SUBKEY);
    let value_name = wide(CLOUD_CODE_URL);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;

    for _ in 0..2 {
        let mut value_type = 0;
        let mut byte_count = 0;
        // Win32 注册表 API 用返回码报告失败，不能依赖线程最后错误。
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value_name.as_ptr(),
                flags,
                &mut value_type,
                std::ptr::null_mut(),
                &mut byte_count,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 {
            return Err(registry_error(
                "读取 Windows 用户级 CLOUD_CODE_URL 失败",
                status,
            ));
        }
        if byte_count > MAX_USER_ENVIRONMENT_VALUE_BYTES || byte_count % 2 != 0 {
            return Err(HostIntegrationError::InvalidIntegration(
                "Windows 用户级 CLOUD_CODE_URL 长度无效".to_string(),
            ));
        }
        if byte_count == 0 {
            return Ok(Some(RegistryStringValue {
                value: String::new(),
                kind: RegistryStringKind::from_registry_type(value_type)?,
            }));
        }

        let mut buffer = vec![0u16; byte_count as usize / 2];
        let mut actual_byte_count = byte_count;
        // 缓冲区长度已由上一轮查询获得；若并发变更，下一轮会重新读取。
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value_name.as_ptr(),
                flags,
                &mut value_type,
                buffer.as_mut_ptr().cast(),
                &mut actual_byte_count,
            )
        };
        if status == ERROR_MORE_DATA {
            continue;
        }
        if status != 0 {
            return Err(registry_error(
                "读取 Windows 用户级 CLOUD_CODE_URL 失败",
                status,
            ));
        }
        if actual_byte_count > MAX_USER_ENVIRONMENT_VALUE_BYTES || actual_byte_count % 2 != 0 {
            return Err(HostIntegrationError::InvalidIntegration(
                "Windows 用户级 CLOUD_CODE_URL 长度无效".to_string(),
            ));
        }

        let text = String::from_utf16(&buffer[..actual_byte_count as usize / 2])
            .map_err(|_| {
                HostIntegrationError::InvalidIntegration(
                    "Windows 用户级 CLOUD_CODE_URL 不是有效 UTF-16 字符串".to_string(),
                )
            })?
            .trim_end_matches('\0')
            .to_string();
        return Ok(Some(RegistryStringValue {
            value: text,
            kind: RegistryStringKind::from_registry_type(value_type)?,
        }));
    }

    Err(HostIntegrationError::InvalidIntegration(
        "Windows 用户级 CLOUD_CODE_URL 在读取过程中持续变化".to_string(),
    ))
}

pub(super) fn write_user_environment_value(
    value: &RegistryStringValue,
) -> Result<(), HostIntegrationError> {
    let process_value = process_environment_value(value)?;
    let subkey = wide(USER_ENVIRONMENT_SUBKEY);
    let value_name = wide(CLOUD_CODE_URL);
    let data = utf16_bytes(&wide(&value.value));
    // 直接写入 HKCU，避免拼接 PowerShell 或命令行参数。
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value_name.as_ptr(),
            value.kind.registry_type(),
            data.as_ptr().cast(),
            data.len() as u32,
        )
    };
    if status != 0 {
        return Err(registry_error(
            "写入 Windows 用户级 CLOUD_CODE_URL 失败",
            status,
        ));
    }
    // Tauri 随后直接启动的 App 会继承当前进程环境，不能只依赖 Explorer 的异步刷新。
    std::env::set_var(CLOUD_CODE_URL, process_value);
    notify_user_environment_change();
    Ok(())
}

pub(super) fn delete_user_environment_value() -> Result<(), HostIntegrationError> {
    let subkey = wide(USER_ENVIRONMENT_SUBKEY);
    let value_name = wide(CLOUD_CODE_URL);
    // 仅在最后一个 owner 仍持有当前值时才会走到删除分支。
    let status =
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), value_name.as_ptr()) };
    if status != 0 && status != ERROR_FILE_NOT_FOUND {
        return Err(registry_error(
            "删除 Windows 用户级 CLOUD_CODE_URL 失败",
            status,
        ));
    }
    // 最后一个 owner 停用后同步移除桌面进程内的代理地址。
    std::env::remove_var(CLOUD_CODE_URL);
    notify_user_environment_change();
    Ok(())
}

fn notify_user_environment_change() {
    let environment = wide(USER_ENVIRONMENT_SUBKEY);
    // 注册表写入已经成功；广播仅用于让已运行的 Windows 外壳刷新环境缓存。
    let _ = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5_000,
            std::ptr::null_mut(),
        )
    };
}

fn registry_error(action: &str, code: u32) -> HostIntegrationError {
    HostIntegrationError::InvalidIntegration(format!(
        "{action}: {}",
        std::io::Error::from_raw_os_error(code as i32)
    ))
}

fn process_environment_value(value: &RegistryStringValue) -> Result<String, HostIntegrationError> {
    match value.kind {
        RegistryStringKind::String => Ok(value.value.clone()),
        RegistryStringKind::ExpandableString => expand_environment_string(&value.value),
    }
}

fn expand_environment_string(value: &str) -> Result<String, HostIntegrationError> {
    let source = wide(value);
    let required = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
    if required == 0 || required > MAX_USER_ENVIRONMENT_VALUE_BYTES / 2 {
        return Err(HostIntegrationError::InvalidIntegration(format!(
            "展开 Windows 用户级 CLOUD_CODE_URL 失败：{}",
            std::io::Error::last_os_error()
        )));
    }

    let mut buffer = vec![0_u16; required as usize];
    let written = unsafe {
        ExpandEnvironmentStringsW(source.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
    };
    if written == 0 || written > buffer.len() as u32 {
        return Err(HostIntegrationError::InvalidIntegration(format!(
            "展开 Windows 用户级 CLOUD_CODE_URL 失败：{}",
            std::io::Error::last_os_error()
        )));
    }

    let text = buffer
        .get(..written.saturating_sub(1) as usize)
        .ok_or_else(|| {
            HostIntegrationError::InvalidIntegration(
                "展开后的 Windows 用户级 CLOUD_CODE_URL 长度无效".to_string(),
            )
        })?;
    String::from_utf16(text).map_err(|_| {
        HostIntegrationError::InvalidIntegration(
            "展开后的 Windows 用户级 CLOUD_CODE_URL 不是有效 UTF-16 字符串".to_string(),
        )
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn utf16_bytes(value: &[u16]) -> Vec<u8> {
    value.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_environment_value_preserves_plain_strings() {
        let value = RegistryStringValue {
            value: "%SystemRoot%".to_string(),
            kind: RegistryStringKind::String,
        };

        assert_eq!(process_environment_value(&value).unwrap(), "%SystemRoot%");
    }

    #[test]
    fn process_environment_value_expands_expandable_strings() {
        let value = RegistryStringValue {
            value: "%SystemRoot%\\System32".to_string(),
            kind: RegistryStringKind::ExpandableString,
        };

        let expanded = process_environment_value(&value).unwrap();
        assert!(!expanded.contains("%SystemRoot%"));
        assert!(expanded.ends_with("\\System32"));
    }
}
