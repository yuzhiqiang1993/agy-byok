use super::{ownership, patch, CliIntegrationState, CliIntegrationStatus};
use crate::error::HostIntegrationError;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn user_home_dir() -> Option<PathBuf> {
    std::env::var("HOME").map(PathBuf::from).ok()
}

pub(super) fn detect_cli_path() -> Option<PathBuf> {
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

pub(super) fn inspect_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let cli_path = super::detect_cli_path();
    let installed = cli_path.is_some();

    let home = match super::user_home_dir() {
        Some(h) => h,
        None => {
            return Ok(CliIntegrationStatus {
                installed,
                state: CliIntegrationState::Disabled,
                cli_path,
                configured_endpoint: None,
                has_ownership: false,
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
            if let Some(ep) = patch::extract_endpoint_from_content(&content) {
                if detected_endpoint.is_none() {
                    detected_endpoint = Some(ep.clone());
                }
                updated_files.push(file.clone());
            }
        }
    }

    let ownership_path = integration_root.join(super::CLI_OWNERSHIP_FILE);
    let ownership = ownership::read_ownership_if_present(&ownership_path)?;

    let current_env_ep = std::env::var("CLOUD_CODE_URL").ok();

    let has_ownership = ownership.is_some();
    let state = if let Some(ref ep) = detected_endpoint {
        if ep == target_endpoint && has_ownership {
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
        has_ownership,
        endpoint_matches,
        shell_configs_updated: updated_files,
        message,
    })
}

pub(super) fn candidate_shell_configs(home: &Path) -> Vec<PathBuf> {
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

pub(super) fn target_shell_configs_for_write(home: &Path) -> Vec<PathBuf> {
    let existing = candidate_shell_configs(home);
    if !existing.is_empty() {
        return existing;
    }
    vec![home.join(".zshrc")]
}
