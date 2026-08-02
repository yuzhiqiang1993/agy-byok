use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const HOST_RESTART_TIMEOUT: Duration = Duration::from_secs(15);
const HOST_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn wait_for_app_state(
    app_path: &Path,
    label: &str,
    expected_running: bool,
) -> Result<(), String> {
    wait_for_process_state(&resolve_host_executable(app_path), label, expected_running)
}

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

pub fn is_app_running(app_path: &Path, label: &str) -> Result<bool, String> {
    is_process_running(&resolve_host_executable(app_path), label)
}

pub fn is_process_running(executable: &Path, label: &str) -> Result<bool, String> {
    let executable_text = executable.display().to_string();
    let pattern = format!("^{}( |$)", escape_pgrep_pattern(&executable_text));
    let status = Command::new("pgrep")
        .args(["-f", &pattern])
        .status()
        .map_err(|error| format!("无法检查 {label} 进程：{error}"))?;
    match status.code() {
        Some(1) => Ok(false),
        Some(0) => Ok(true),
        _ => Err(format!("检查 {label} 进程失败：{status}")),
    }
}

pub fn terminate_process(executable: &Path, label: &str) -> Result<(), String> {
    if !is_process_running(executable, label)? {
        return Ok(());
    }

    let pattern = format!(
        "^{}( |$)",
        escape_pgrep_pattern(&executable.display().to_string())
    );
    let status = Command::new("pkill")
        .args(["-TERM", "-f", &pattern])
        .status()
        .map_err(|error| format!("无法请求 {label} 强制退出：{error}"))?;
    if !matches!(status.code(), Some(0) | Some(1)) {
        return Err(format!("请求 {label} 退出失败：{status}"));
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !is_process_running(executable, label)? {
            return Ok(());
        }
        std::thread::sleep(HOST_PROCESS_POLL_INTERVAL);
    }

    let status = Command::new("pkill")
        .args(["-KILL", "-f", &pattern])
        .status()
        .map_err(|error| format!("无法终止 {label}：{error}"))?;
    if !matches!(status.code(), Some(0) | Some(1)) {
        return Err(format!("终止 {label} 失败：{status}"));
    }
    wait_for_process_state(executable, label, false)
}

pub fn resolve_host_executable(app_path: &Path) -> PathBuf {
    let macos_dir = app_path.join("Contents/MacOS");
    let mut candidates = vec![macos_dir.join("Electron")];
    if let Some(bundle_name) = app_path.file_stem() {
        candidates.push(macos_dir.join(bundle_name));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| macos_dir.join("Electron"))
}

pub fn escape_pgrep_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if ".^$*+?()[]{}|\\".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

pub fn command_argument(command_line: &str, name: &str) -> Option<String> {
    let mut parts = command_line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == name {
            return parts.next().map(ToString::to_string);
        }
        if let Some(value) = part.strip_prefix(&format!("{name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_argument_supports_separate_and_equals_forms() {
        assert_eq!(
            command_argument(
                "language_server --cloud_code_endpoint http://127.0.0.1:57134",
                "--cloud_code_endpoint",
            ),
            Some("http://127.0.0.1:57134".to_string())
        );
        assert_eq!(
            command_argument(
                "language_server --cloud_code_endpoint=http://127.0.0.1:57134",
                "--cloud_code_endpoint",
            ),
            Some("http://127.0.0.1:57134".to_string())
        );
        assert_eq!(
            command_argument("language_server --other value", "--cloud_code_endpoint"),
            None
        );
    }
}
