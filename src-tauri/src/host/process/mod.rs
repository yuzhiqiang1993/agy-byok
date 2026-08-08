use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as current;
#[cfg(target_os = "windows")]
use windows as current;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod current {
    use std::path::Path;

    pub(super) fn is_process_running(_executable: &Path, label: &str) -> Result<bool, String> {
        Err(format!("当前平台不支持检查 {label} 进程"))
    }

    pub(super) fn terminate_process(_executable: &Path, label: &str) -> Result<(), String> {
        Err(format!("当前平台不支持结束 {label} 进程"))
    }

    pub(super) fn launch_application_with_environment(
        _installation: &Path,
        _executable: &Path,
        label: &str,
        _environment: &[(&str, &str)],
    ) -> Result<(), String> {
        Err(format!("当前平台不支持启动 {label}"))
    }
}

const HOST_RESTART_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn wait_for_process_state(
    executable: &Path,
    label: &str,
    expected_running: bool,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < HOST_RESTART_TIMEOUT {
        if is_process_running(executable, label)? == expected_running {
            return Ok(());
        }
        std::thread::sleep(HOST_PROCESS_POLL_INTERVAL);
    }

    let expected = if expected_running { "启动" } else { "退出" };
    Err(format!(
        "等待 {label} {expected}超时（{} 秒）",
        HOST_RESTART_TIMEOUT.as_secs()
    ))
}

pub fn is_process_running(executable: &Path, label: &str) -> Result<bool, String> {
    current::is_process_running(executable, label)
}

pub fn terminate_process(executable: &Path, label: &str) -> Result<(), String> {
    current::terminate_process(executable, label)
}

pub fn launch_application(
    installation: &Path,
    executable: &Path,
    label: &str,
) -> Result<(), String> {
    launch_application_with_environment(installation, executable, label, &[])
}

pub fn launch_application_with_environment(
    installation: &Path,
    executable: &Path,
    label: &str,
    environment: &[(&str, &str)],
) -> Result<(), String> {
    current::launch_application_with_environment(installation, executable, label, environment)
}
