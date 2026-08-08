use super::{wait_for_process_state, HOST_PROCESS_POLL_INTERVAL};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub(super) fn is_process_running(executable: &Path, label: &str) -> Result<bool, String> {
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

pub(super) fn terminate_process(executable: &Path, label: &str) -> Result<(), String> {
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

pub(super) fn launch_application_with_environment(
    installation: &Path,
    _executable: &Path,
    label: &str,
    environment: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = Command::new("/usr/bin/open");
    for (name, value) in environment {
        command.arg("--env").arg(format!("{name}={value}"));
    }
    command.arg(installation);
    let status = command
        .status()
        .map_err(|error| format!("无法启动 {label}：{error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("启动 {label} 失败：{status}"))
}

fn escape_pgrep_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if ".^$*+?()[]{}|\\".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_pattern_escapes_regular_expression_metacharacters() {
        assert_eq!(escape_pgrep_pattern("/Apps/A.B+"), "/Apps/A\\.B\\+");
    }
}
