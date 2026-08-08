use super::{wait_for_process_state, HOST_PROCESS_POLL_INTERVAL};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

const MAX_WINDOWS_PATH_LENGTH: usize = 32_768;

pub(super) fn is_process_running(executable: &Path, label: &str) -> Result<bool, String> {
    Ok(!matching_process_ids(executable, label)?.is_empty())
}

pub(super) fn terminate_process(executable: &Path, label: &str) -> Result<(), String> {
    let process_ids = matching_process_ids(executable, label)?;
    if process_ids.is_empty() {
        return Ok(());
    }

    for process_id in process_ids {
        request_process_exit(process_id, false, label)?;
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !is_process_running(executable, label)? {
            return Ok(());
        }
        std::thread::sleep(HOST_PROCESS_POLL_INTERVAL);
    }

    for process_id in matching_process_ids(executable, label)? {
        request_process_exit(process_id, true, label)?;
    }
    wait_for_process_state(executable, label, false)
}

fn request_process_exit(process_id: u32, force: bool, label: &str) -> Result<(), String> {
    let mut command = Command::new("taskkill");
    if force {
        command.arg("/F");
    }
    command.args(["/PID", &process_id.to_string(), "/T"]);
    // 进程可能在枚举后自行退出；最终以完整路径复检结果为准。
    command
        .status()
        .map(|_| ())
        .map_err(|error| format!("无法结束 {label} 进程 {process_id}：{error}"))
}

fn matching_process_ids(executable: &Path, label: &str) -> Result<Vec<u32>, String> {
    let expected_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("无法识别 {label} 的 Windows 可执行文件名"))?;
    let expected_path = executable.to_path_buf();
    let snapshot = OwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
        .ok_or_else(|| format!("无法枚举 {label} 进程：{}", std::io::Error::last_os_error()))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    if unsafe { Process32FirstW(snapshot.get(), &mut entry) } == 0 {
        return Err(format!(
            "无法读取 {label} 进程列表：{}",
            std::io::Error::last_os_error()
        ));
    }

    let mut process_ids = Vec::new();
    loop {
        let image_name = wide_array_string(&entry.szExeFile);
        if image_name.eq_ignore_ascii_case(expected_name) {
            if let Some(actual_path) = process_executable_path(entry.th32ProcessID) {
                if windows_paths_equal(&actual_path, &expected_path) {
                    process_ids.push(entry.th32ProcessID);
                }
            }
        }
        if unsafe { Process32NextW(snapshot.get(), &mut entry) } == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_NO_MORE_FILES {
                return Err(format!(
                    "枚举 {label} 进程中断：{}",
                    std::io::Error::from_raw_os_error(error as i32)
                ));
            }
            break;
        }
    }
    Ok(process_ids)
}

fn process_executable_path(process_id: u32) -> Option<PathBuf> {
    let process =
        OwnedHandle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) })?;
    let mut buffer = vec![0_u16; MAX_WINDOWS_PATH_LENGTH];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.get(), 0, buffer.as_mut_ptr(), &mut length) }
        == 0
    {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

fn wide_array_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn windows_paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .replace('/', "\\")
        .eq_ignore_ascii_case(&right.to_string_lossy().replace('/', "\\"))
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
    }

    fn get(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(super) fn launch_application_with_environment(
    _installation: &Path,
    executable: &Path,
    label: &str,
    environment: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = Command::new(executable);
    for (name, value) in environment {
        command.env(name, value);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动 {label}：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_path_match_is_case_and_separator_insensitive() {
        assert!(windows_paths_equal(
            Path::new(r"C:\Users\Demo\Antigravity.exe"),
            Path::new("c:/users/demo/ANTIGRAVITY.EXE"),
        ));
        assert!(!windows_paths_equal(
            Path::new(r"C:\Tools\Antigravity.exe"),
            Path::new(r"C:\Users\Demo\Antigravity.exe"),
        ));
    }
}
