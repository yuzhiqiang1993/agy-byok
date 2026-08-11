use super::{wait_for_process_state, HOST_PROCESS_POLL_INTERVAL};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub(super) fn is_process_running(executable: &Path, label: &str) -> Result<bool, String> {
    let pids = get_process_pids(executable, label)?;
    Ok(!pids.is_empty())
}

pub(super) fn terminate_process(executable: &Path, label: &str) -> Result<(), String> {
    let pids = get_process_pids(executable, label)?;
    if pids.is_empty() {
        return Ok(());
    }

    for pid in &pids {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !is_process_running(executable, label)? {
            return Ok(());
        }
        std::thread::sleep(HOST_PROCESS_POLL_INTERVAL);
    }

    let remaining_pids = get_process_pids(executable, label)?;
    for pid in &remaining_pids {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    wait_for_process_state(executable, label, false)
}

fn get_process_pids(executable: &Path, label: &str) -> Result<Vec<u32>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid,command"])
        .output()
        .map_err(|error| format!("无法检查 {label} 进程：{error}"))?;
    if !output.status.success() {
        return Err(format!("检查 {label} 进程失败：{}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_matching_pids(&stdout, executable))
}

fn parse_matching_pids(stdout: &str, executable: &Path) -> Vec<u32> {
    let executable_text = executable.display().to_string();
    let mut pids = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some((pid_str, cmd)) = trimmed.split_once(' ') {
            let cmd = cmd.trim_start();
            if cmd == executable_text || cmd.starts_with(&format!("{} ", executable_text)) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

pub(super) fn launch_application_with_environment(
    installation: &Path,
    _executable: &Path,
    label: &str,
    environment: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = build_open_command(installation, environment);
    let status = command
        .status()
        .map_err(|error| format!("无法启动 {label}：{error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("启动 {label} 失败：{status}"))
}

fn build_open_command(installation: &Path, environment: &[(&str, &str)]) -> Command {
    let mut command = Command::new("open");
    command.arg("-a");
    command.arg(installation);
    for (name, value) in environment {
        command.arg("--env");
        command.arg(format!("{name}={value}"));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_matching_pids_from_ps_output() {
        let stdout = " 1465 /Applications/Antigravity IDE.app/Contents/MacOS/Electron\n 1468 /Applications/Antigravity IDE.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler\n 9494 /Applications/Antigravity IDE.app/Contents/Frameworks/Antigravity IDE Helper (Renderer).app/Contents/MacOS/Antigravity IDE Helper (Renderer)\n";
        let pids = parse_matching_pids(
            stdout,
            Path::new("/Applications/Antigravity IDE.app/Contents/MacOS/Electron"),
        );
        assert_eq!(pids, vec![1465]);
    }

    #[test]
    fn build_open_command_places_application_path_immediately_after_dash_a() {
        let installation = Path::new("/Applications/Antigravity.app");
        let environment = [("CLOUD_CODE_URL", "http://127.0.0.1:12345")];
        let command = build_open_command(installation, &environment);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "-a",
                "/Applications/Antigravity.app",
                "--env",
                "CLOUD_CODE_URL=http://127.0.0.1:12345"
            ]
        );
    }
}
