use super::{CLI_FISH_MARKER_BEGIN, CLI_FISH_MARKER_END, CLI_MARKER_BEGIN, CLI_MARKER_END};
use crate::error::{io_error, HostIntegrationError};
use std::fs;
use std::path::Path;

pub(super) fn is_fish_config(file: &Path) -> bool {
    file.extension().and_then(|e| e.to_str()) == Some("fish")
        || file.to_string_lossy().contains("fish")
}

pub(super) fn snippet_for(endpoint: &str, is_fish: bool) -> String {
    if is_fish {
        format!(
            "{CLI_FISH_MARKER_BEGIN}\nset -gx CLOUD_CODE_URL \"{endpoint}\"\n{CLI_FISH_MARKER_END}\n"
        )
    } else {
        format!("{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"{endpoint}\"\n{CLI_MARKER_END}\n")
    }
}

pub(super) fn extract_endpoint_from_content(content: &str) -> Option<String> {
    let mut endpoint = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("export CLOUD_CODE_URL=") {
            let val = trimmed.trim_start_matches("export CLOUD_CODE_URL=").trim();
            let val = val.trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                endpoint = Some(val.to_string());
            }
        } else if trimmed.starts_with("set -gx CLOUD_CODE_URL") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let val = parts[3].trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    endpoint = Some(val.to_string());
                }
            }
        }
    }
    endpoint
}

pub(super) fn update_shell_config_file(
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

pub(super) fn remove_snippet_from_file(
    file: &Path,
    is_fish: bool,
) -> Result<(), HostIntegrationError> {
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

pub(super) fn remove_snippet_lines(content: &str, is_fish: bool) -> String {
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

pub(super) fn validate_local_endpoint(endpoint: &str) -> Result<(), HostIntegrationError> {
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
