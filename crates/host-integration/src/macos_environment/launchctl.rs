use crate::error::HostIntegrationError;
use std::process::{Command, Output};

const CLOUD_CODE_URL: &str = "CLOUD_CODE_URL";

pub(super) fn read_endpoint() -> Result<Option<String>, HostIntegrationError> {
    let output = run(["getenv", CLOUD_CODE_URL])?;
    ensure_success("读取 CLOUD_CODE_URL", &output)?;
    let value = String::from_utf8(output.stdout).map_err(|error| {
        HostIntegrationError::Command(format!(
            "launchctl 返回了无效 UTF-8 的 CLOUD_CODE_URL：{error}"
        ))
    })?;
    let value = value.trim_end_matches(['\r', '\n']);
    Ok((!value.is_empty()).then(|| value.to_string()))
}

pub(super) fn set_endpoint(endpoint: &str) -> Result<(), HostIntegrationError> {
    let output = run(["setenv", CLOUD_CODE_URL, endpoint])?;
    ensure_success("设置 CLOUD_CODE_URL", &output)
}

pub(super) fn remove_endpoint() -> Result<(), HostIntegrationError> {
    let output = run(["unsetenv", CLOUD_CODE_URL])?;
    ensure_success("移除 CLOUD_CODE_URL", &output)
}

fn run<const N: usize>(arguments: [&str; N]) -> Result<Output, HostIntegrationError> {
    Command::new("/bin/launchctl")
        .args(arguments)
        .output()
        .map_err(|error| HostIntegrationError::Command(format!("无法启动 launchctl：{error}")))
}

fn ensure_success(operation: &str, output: &Output) -> Result<(), HostIntegrationError> {
    if output.status.success() {
        return Ok(());
    }
    let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(HostIntegrationError::Command(format!(
        "launchctl {operation}失败（{}）：{details}",
        output.status
    )))
}
