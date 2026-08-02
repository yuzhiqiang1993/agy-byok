use crate::error::{io_error, HostIntegrationError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CLI_MARKER_BEGIN: &str = "# >>> AGY BYOK CLI Integration >>>";
pub const CLI_MARKER_END: &str = "# <<< AGY BYOK CLI Integration <<<";
pub const CLI_FISH_MARKER_BEGIN: &str = "# >>> AGY BYOK CLI Integration (Fish) >>>";
pub const CLI_FISH_MARKER_END: &str = "# <<< AGY BYOK CLI Integration (Fish) <<<";
pub const CLI_OWNERSHIP_FILE: &str = "cli-ownership.json";
const OWNERSHIP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CliOwnership {
    schema_version: u32,
    managed_endpoint: String,
    updated_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CliIntegrationState {
    Disabled,
    Managed,
    Mismatch,
    External,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CliIntegrationStatus {
    pub installed: bool,
    pub state: CliIntegrationState,
    pub cli_path: Option<PathBuf>,
    pub configured_endpoint: Option<String>,
    pub endpoint_matches: bool,
    pub shell_configs_updated: Vec<PathBuf>,
    pub message: String,
}

pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var("HOME").map(PathBuf::from).ok()
}

pub fn detect_cli_path() -> Option<PathBuf> {
    if let Some(home) = user_home_dir() {
        let local_bin = home.join(".local").join("bin").join("agy");
        if local_bin.is_file() {
            return Some(local_bin);
        }
    }

    let common_paths = [
        PathBuf::from("/usr/local/bin/agy"),
        PathBuf::from("/opt/homebrew/bin/agy"),
    ];

    for path in &common_paths {
        if path.is_file() {
            return Some(path.clone());
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("agy");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let cli_path = detect_cli_path();
    let installed = cli_path.is_some();

    let home = match user_home_dir() {
        Some(h) => h,
        None => {
            return Ok(CliIntegrationStatus {
                installed,
                state: CliIntegrationState::Disabled,
                cli_path,
                configured_endpoint: None,
                endpoint_matches: false,
                shell_configs_updated: Vec::new(),
                message: "未找到用户 Home 目录，无法检查 CLI 配置文件".to_string(),
            });
        }
    };

    let target_files = candidate_shell_configs(&home);
    let mut detected_endpoint: Option<String> = None;
    let mut updated_files = Vec::new();

    for file in &target_files {
        if let Ok(content) = fs::read_to_string(file) {
            if let Some(ep) = extract_endpoint_from_content(&content) {
                if detected_endpoint.is_none() {
                    detected_endpoint = Some(ep.clone());
                }
                updated_files.push(file.clone());
            }
        }
    }

    let ownership_path = integration_root.join(CLI_OWNERSHIP_FILE);
    let ownership = read_ownership_if_present(&ownership_path)?;

    let current_env_ep = std::env::var("CLOUD_CODE_URL").ok();

    let state = if let Some(ref ep) = detected_endpoint {
        if ep == target_endpoint && ownership.is_some() {
            CliIntegrationState::Managed
        } else if ep == target_endpoint {
            CliIntegrationState::External
        } else {
            CliIntegrationState::Mismatch
        }
    } else if let Some(ref env_ep) = current_env_ep {
        if env_ep == target_endpoint {
            CliIntegrationState::External
        } else {
            CliIntegrationState::Mismatch
        }
    } else {
        CliIntegrationState::Disabled
    };

    let endpoint_matches = match &detected_endpoint {
        Some(ep) => ep == target_endpoint,
        None => current_env_ep.as_deref() == Some(target_endpoint),
    };

    let configured_endpoint = detected_endpoint.or(current_env_ep);

    let message = match state {
        CliIntegrationState::Managed => {
            format!("CLI 已接入 AGY BYOK 代理 ({target_endpoint})；Shell 配置文件已自动注入 CLOUD_CODE_URL")
        }
        CliIntegrationState::External => {
            format!(
                "检测到外部 CLOUD_CODE_URL 配置 ({})",
                configured_endpoint.as_deref().unwrap_or(target_endpoint)
            )
        }
        CliIntegrationState::Mismatch => {
            format!(
                "CLI 配置指向其他 Endpoint ({})",
                configured_endpoint.as_deref().unwrap_or("未知")
            )
        }
        CliIntegrationState::Disabled => {
            if installed {
                "Antigravity CLI 已安装，尚未启用本地代理接入".to_string()
            } else {
                "未在系统 PATH 或 ~/.local/bin 中找到 Antigravity CLI (agy)".to_string()
            }
        }
    };

    Ok(CliIntegrationStatus {
        installed,
        state,
        cli_path,
        configured_endpoint,
        endpoint_matches,
        shell_configs_updated: updated_files,
        message,
    })
}

pub fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    validate_local_endpoint(target_endpoint)?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;

    let home = user_home_dir()
        .ok_or_else(|| HostIntegrationError::InvalidBundle("无法获取 Home 目录".to_string()))?;

    let target_files = target_shell_configs_for_write(&home);
    let mut updated_files = Vec::new();

    for file in &target_files {
        let is_fish = file.extension().and_then(|e| e.to_str()) == Some("fish")
            || file.to_string_lossy().contains("fish");

        let snippet = if is_fish {
            format!(
                "{CLI_FISH_MARKER_BEGIN}\nset -gx CLOUD_CODE_URL \"{target_endpoint}\"\n{CLI_FISH_MARKER_END}\n"
            )
        } else {
            format!(
                "{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"{target_endpoint}\"\n{CLI_MARKER_END}\n"
            )
        };

        update_shell_config_file(file, &snippet, is_fish)?;
        updated_files.push(file.clone());
    }

    let helper_env_path = integration_root.join("cli-integration").join("env.sh");
    if let Some(parent) = helper_env_path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    let helper_content =
        format!("# AGY BYOK CLI Integration Helper\nexport CLOUD_CODE_URL=\"{target_endpoint}\"\n");
    fs::write(&helper_env_path, helper_content).map_err(|e| io_error(&helper_env_path, e))?;

    let ownership = CliOwnership {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        managed_endpoint: target_endpoint.to_string(),
        updated_files: updated_files.clone(),
    };

    let ownership_path = integration_root.join(CLI_OWNERSHIP_FILE);
    let ownership_bytes = serde_json::to_vec_pretty(&ownership).map_err(|e| {
        HostIntegrationError::InvalidBundle(format!("无法序列化 CLI ownership: {e}"))
    })?;
    fs::write(&ownership_path, ownership_bytes).map_err(|e| io_error(&ownership_path, e))?;

    inspect_cli_integration(integration_root, target_endpoint)
}

pub fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let home = user_home_dir()
        .ok_or_else(|| HostIntegrationError::InvalidBundle("无法获取 Home 目录".to_string()))?;

    let candidate_files = candidate_shell_configs(&home);
    for file in &candidate_files {
        if file.is_file() {
            let is_fish = file.extension().and_then(|e| e.to_str()) == Some("fish")
                || file.to_string_lossy().contains("fish");
            remove_snippet_from_file(file, is_fish)?;
        }
    }

    let ownership_path = integration_root.join(CLI_OWNERSHIP_FILE);
    if ownership_path.is_file() {
        let _ = fs::remove_file(&ownership_path);
    }

    inspect_cli_integration(integration_root, target_endpoint)
}

fn candidate_shell_configs(home: &Path) -> Vec<PathBuf> {
    let mut list = Vec::new();
    let zshrc = home.join(".zshrc");
    if zshrc.is_file() {
        list.push(zshrc);
    }
    let bash_profile = home.join(".bash_profile");
    if bash_profile.is_file() {
        list.push(bash_profile);
    }
    let bashrc = home.join(".bashrc");
    if bashrc.is_file() {
        list.push(bashrc);
    }
    let fish_config = home.join(".config").join("fish").join("config.fish");
    if fish_config.is_file() {
        list.push(fish_config);
    }
    list
}

fn target_shell_configs_for_write(home: &Path) -> Vec<PathBuf> {
    let existing = candidate_shell_configs(home);
    if !existing.is_empty() {
        return existing;
    }
    vec![home.join(".zshrc")]
}

fn extract_endpoint_from_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("export CLOUD_CODE_URL=") {
            let val = trimmed.trim_start_matches("export CLOUD_CODE_URL=").trim();
            let val = val.trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        } else if trimmed.starts_with("set -gx CLOUD_CODE_URL") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let val = parts[3].trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn update_shell_config_file(
    file: &Path,
    snippet: &str,
    is_fish: bool,
) -> Result<(), HostIntegrationError> {
    let content = if file.is_file() {
        fs::read_to_string(file).map_err(|e| io_error(file, e))?
    } else {
        String::new()
    };

    let cleaned = remove_snippet_lines(&content, is_fish);
    let mut new_content = cleaned.trim_end().to_string();
    if !new_content.is_empty() {
        new_content.push('\n');
        new_content.push('\n');
    }
    new_content.push_str(snippet);

    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }

    fs::write(file, new_content).map_err(|e| io_error(file, e))
}

fn remove_snippet_from_file(file: &Path, is_fish: bool) -> Result<(), HostIntegrationError> {
    if !file.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(file).map_err(|e| io_error(file, e))?;
    let cleaned = remove_snippet_lines(&content, is_fish);
    if cleaned != content {
        fs::write(file, &cleaned).map_err(|e| io_error(file, e))?;
    }
    Ok(())
}

fn remove_snippet_lines(content: &str, is_fish: bool) -> String {
    let begin_marker = if is_fish {
        CLI_FISH_MARKER_BEGIN
    } else {
        CLI_MARKER_BEGIN
    };
    let end_marker = if is_fish {
        CLI_FISH_MARKER_END
    } else {
        CLI_MARKER_END
    };

    let mut result = Vec::new();
    let mut skipping = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == begin_marker {
            skipping = true;
            continue;
        }
        if trimmed == end_marker {
            skipping = false;
            continue;
        }
        if !skipping {
            result.push(line);
        }
    }

    let mut res = result.join("\n");
    if content.ends_with('\n') && !res.is_empty() {
        res.push('\n');
    }
    res
}

fn read_ownership_if_present(
    ownership_path: &Path,
) -> Result<Option<CliOwnership>, HostIntegrationError> {
    if !ownership_path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(ownership_path).map_err(|e| io_error(ownership_path, e))?;
    let ownership: CliOwnership = serde_json::from_slice(&bytes).map_err(|e| {
        HostIntegrationError::InvalidBundle(format!("无法解析 CLI ownership 格式: {e}"))
    })?;
    if ownership.schema_version == OWNERSHIP_SCHEMA_VERSION {
        Ok(Some(ownership))
    } else {
        Ok(None)
    }
}

fn validate_local_endpoint(endpoint: &str) -> Result<(), HostIntegrationError> {
    let Some(port) = endpoint.strip_prefix("http://127.0.0.1:") else {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "CLI 接入只允许使用本地代理地址，收到 {endpoint}"
        )));
    };
    let valid_port = !port.is_empty()
        && port.chars().all(|character| character.is_ascii_digit())
        && port.parse::<u16>().map(|value| value > 0).unwrap_or(false);
    if !valid_port {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "CLI 接入端口无效：{endpoint}"
        )));
    }
    Ok(())
}

fn prepare_private_directory(path: &Path) -> Result<(), HostIntegrationError> {
    fs::create_dir_all(path).map_err(|e| io_error(path, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_snippet_insertion_and_removal() {
        let content = "export PATH=$PATH:~/.local/bin\n";
        let snippet = format!("{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"http://127.0.0.1:51234\"\n{CLI_MARKER_END}\n");

        let updated = format!("{content}\n{snippet}");
        assert!(updated.contains(CLI_MARKER_BEGIN));
        assert!(updated.contains("CLOUD_CODE_URL=\"http://127.0.0.1:51234\""));

        let cleaned = remove_snippet_lines(&updated, false);
        assert!(!cleaned.contains(CLI_MARKER_BEGIN));
        assert!(!cleaned.contains("CLOUD_CODE_URL"));
        assert!(cleaned.contains("export PATH=$PATH:~/.local/bin"));
    }

    #[test]
    fn test_enable_and_disable_cli_integration() {
        let temp_dir = TempDir::new().unwrap();
        let zshrc = temp_dir.path().join(".zshrc");
        fs::write(&zshrc, "# User zshrc\n").unwrap();

        let endpoint = "http://127.0.0.1:51234";
        let is_fish = false;
        let snippet =
            format!("{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"{endpoint}\"\n{CLI_MARKER_END}\n");
        update_shell_config_file(&zshrc, &snippet, is_fish).unwrap();

        let read_back = fs::read_to_string(&zshrc).unwrap();
        assert!(read_back.contains(CLI_MARKER_BEGIN));
        assert_eq!(
            extract_endpoint_from_content(&read_back),
            Some(endpoint.to_string())
        );

        remove_snippet_from_file(&zshrc, is_fish).unwrap();
        let cleaned_back = fs::read_to_string(&zshrc).unwrap();
        assert!(!cleaned_back.contains(CLI_MARKER_BEGIN));
    }
}
