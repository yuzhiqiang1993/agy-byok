mod atomic_file;
mod jsonc_editor;
mod ownership;
#[cfg(test)]
mod tests;

use crate::error::HostIntegrationError;
use crate::local_endpoint::is_local_proxy_endpoint;
use serde_json::Value;
use std::path::Path;

pub(super) const IDE_CLOUD_CODE_SETTING: &str = "jetski.cloudCodeUrl";
pub(super) const IDE_SETTING_OWNERSHIP_FILE: &str = "ide-setting-ownership.json";
const OWNERSHIP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeSettingsState {
    Disabled,
    Managed,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeSettingsStatus {
    pub state: IdeSettingsState,
    pub endpoint_matches: bool,
}

pub fn inspect_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = atomic_file::validate_settings_path(settings_path.as_ref())?;
    let configured_endpoint = match atomic_file::read_optional_regular_file(&settings_path)? {
        Some(bytes) => jsonc_editor::cloud_code_value(&bytes)?
            .and_then(|value| value.as_str().map(str::to_string)),
        None => None,
    };
    let state = configured_setting_state(
        &settings_path,
        integration_root.as_ref(),
        configured_endpoint.as_deref(),
        endpoint,
    )?;
    Ok(IdeSettingsStatus {
        state,
        endpoint_matches: configured_endpoint.as_deref() == Some(endpoint),
    })
}

pub fn enable_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = atomic_file::validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    let current = atomic_file::read_optional_regular_file(&settings_path)?;
    let current_bytes = current.unwrap_or_else(|| b"{}\n".to_vec());
    let current_value = jsonc_editor::cloud_code_value(&current_bytes)?;
    if current_value.as_ref().and_then(Value::as_str) == Some(endpoint) {
        let state =
            configured_setting_state(&settings_path, integration_root, Some(endpoint), endpoint)?;
        return Ok(IdeSettingsStatus {
            state,
            endpoint_matches: true,
        });
    }

    atomic_file::prepare_private_directory(integration_root)?;
    let current_trailing_comma = jsonc_editor::settings_trailing_comma(&current_bytes)?;
    let ownership_path = integration_root.join(IDE_SETTING_OWNERSHIP_FILE);
    let previous_ownership = ownership::read_ownership_if_present(&ownership_path, &settings_path)?;
    let (previous_value, previous_trailing_comma) = match previous_ownership.as_ref() {
        Some(ownership)
            if current_value.as_ref().and_then(Value::as_str)
                == Some(ownership.managed_endpoint.as_str()) =>
        {
            (
                ownership.previous_value.clone(),
                ownership.previous_trailing_comma,
            )
        }
        _ => (current_value, current_trailing_comma),
    };
    let ownership = ownership::IdeSettingOwnership {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        settings_path: settings_path.clone(),
        managed_endpoint: endpoint.to_string(),
        previous_value,
        previous_trailing_comma,
    };
    let configured = jsonc_editor::configure_settings(&current_bytes, endpoint)?;
    atomic_file::write_json_private(&ownership_path, &ownership)?;
    if let Err(settings_error) = atomic_file::write_settings_file(&settings_path, &configured) {
        if let Err(recovery_error) =
            ownership::restore_after_failed_enable(&ownership_path, previous_ownership.as_ref())
        {
            return Err(HostIntegrationError::RecoveryFailed {
                operation: settings_error.to_string(),
                recovery: recovery_error.to_string(),
            });
        }
        return Err(settings_error);
    }
    Ok(managed_status())
}

pub fn disable_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = atomic_file::validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    atomic_file::validate_integration_root_if_present(integration_root)?;
    let Some(current) = atomic_file::read_optional_regular_file(&settings_path)? else {
        return Ok(disabled_status());
    };
    let current_value = jsonc_editor::cloud_code_value(&current)?;
    let Some(configured_endpoint) = current_value.as_ref().and_then(Value::as_str) else {
        return Ok(disabled_status());
    };
    let ownership_path = integration_root.join(IDE_SETTING_OWNERSHIP_FILE);
    let ownership = ownership::read_ownership_if_present(&ownership_path, &settings_path)?;
    let ownership = match ownership {
        Some(ownership) if ownership.managed_endpoint == configured_endpoint => ownership,
        _ if configured_endpoint == endpoint || is_local_proxy_endpoint(configured_endpoint) => {
            let updated = jsonc_editor::remove_setting(&current, false)?;
            atomic_file::write_settings_file(&settings_path, &updated)?;
            return Ok(disabled_status());
        }
        _ => return Ok(disabled_status()),
    };
    let updated = match ownership.previous_value.as_ref() {
        Some(previous_value) => jsonc_editor::configure_setting_value(&current, previous_value)?,
        None => jsonc_editor::remove_setting(&current, ownership.previous_trailing_comma)?,
    };
    atomic_file::write_settings_file(&settings_path, &updated)?;
    if let Err(ownership_error) = atomic_file::remove_regular_file(&ownership_path) {
        if let Err(recovery_error) = atomic_file::write_settings_file(&settings_path, &current) {
            return Err(HostIntegrationError::RecoveryFailed {
                operation: ownership_error.to_string(),
                recovery: recovery_error.to_string(),
            });
        }
        return Err(ownership_error);
    }
    Ok(disabled_status())
}

fn managed_status() -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::Managed,
        endpoint_matches: true,
    }
}

fn disabled_status() -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::Disabled,
        endpoint_matches: false,
    }
}

#[allow(dead_code)]
fn external_status(endpoint_matches: bool) -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::External,
        endpoint_matches,
    }
}

fn configured_setting_state(
    settings_path: &Path,
    integration_root: &Path,
    configured_endpoint: Option<&str>,
    current_endpoint: &str,
) -> Result<IdeSettingsState, HostIntegrationError> {
    let Some(configured_endpoint) = configured_endpoint else {
        return Ok(IdeSettingsState::Disabled);
    };
    atomic_file::validate_integration_root_if_present(integration_root)?;
    let ownership_path = integration_root.join(IDE_SETTING_OWNERSHIP_FILE);
    let ownership = ownership::read_ownership_if_present(&ownership_path, settings_path)?;
    if ownership
        .as_ref()
        .is_some_and(|record| record.managed_endpoint == configured_endpoint)
    {
        return Ok(IdeSettingsState::Managed);
    }
    if configured_endpoint == current_endpoint || is_local_proxy_endpoint(configured_endpoint) {
        Ok(IdeSettingsState::External)
    } else {
        Ok(IdeSettingsState::Disabled)
    }
}

fn settings_conflict(message: impl Into<String>) -> HostIntegrationError {
    HostIntegrationError::SettingsConflict(message.into())
}
