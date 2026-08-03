use super::{ENDPOINT_MARKER, TARGET_OFFICIAL_ENDPOINT, WRAPPER_MARKER};
use crate::error::HostIntegrationError;

pub(super) fn validate_local_endpoint(endpoint: &str) -> Result<(), HostIntegrationError> {
    let Some(port) = endpoint.strip_prefix("http://127.0.0.1:") else {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "App 接入只允许使用本地代理地址，收到 {endpoint}"
        )));
    };
    let valid_port = !port.is_empty()
        && port.chars().all(|character| character.is_ascii_digit())
        && port.parse::<u16>().map(|value| value > 0).unwrap_or(false);
    if !valid_port {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "App 接入端口无效：{endpoint}"
        )));
    }
    Ok(())
}

pub(super) fn wrapper_script(endpoint: &str) -> String {
    format!(
        r#"#!/bin/bash
{WRAPPER_MARKER}
{ENDPOINT_MARKER}{endpoint}
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
ARGS=()
for arg in "$@"; do
    if [ "$arg" = "{TARGET_OFFICIAL_ENDPOINT}" ]; then
        ARGS+=("{endpoint}")
    else
        ARGS+=("$arg")
    fi
done
exec "$DIR/language_server.real" "${{ARGS[@]}}"
"#
    )
}

pub(super) fn is_managed_wrapper(content: &str) -> bool {
    content.lines().any(|line| line.trim() == WRAPPER_MARKER)
}

pub(super) fn is_managed_wrapper_bytes(bytes: &[u8]) -> bool {
    String::from_utf8(bytes.to_vec())
        .map(|content| {
            is_managed_wrapper(&content) || legacy_endpoint_from_wrapper(&content).is_some()
        })
        .unwrap_or(false)
}

pub(super) fn endpoint_from_wrapper(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(ENDPOINT_MARKER).map(str::to_string))
}

pub(super) fn legacy_endpoint_from_wrapper(content: &str) -> Option<String> {
    let expected_structure = content.contains("#!/bin/bash")
        && content.contains("DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"")
        && content.contains("if [ \"$arg\" = \"")
        && content.contains(TARGET_OFFICIAL_ENDPOINT)
        && content.contains("exec \"$DIR/language_server.real\"");
    if !expected_structure {
        return None;
    }

    content.lines().find_map(|line| {
        let value = line.trim().strip_prefix("ARGS+=(\"")?.strip_suffix("\")")?;
        if value == "$arg" || validate_local_endpoint(value).is_err() {
            None
        } else {
            Some(value.to_string())
        }
    })
}
