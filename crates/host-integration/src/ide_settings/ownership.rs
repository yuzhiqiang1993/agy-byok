use super::atomic_file;
use super::{settings_conflict, OWNERSHIP_SCHEMA_VERSION};
use crate::error::HostIntegrationError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct IdeSettingOwnership {
    pub(super) schema_version: u32,
    pub(super) settings_path: PathBuf,
    pub(super) managed_endpoint: String,
    #[serde(deserialize_with = "crate::serde_helpers::required_nullable")]
    pub(super) previous_value: Option<Value>,
    pub(super) previous_trailing_comma: bool,
}

pub(super) fn read_ownership_if_present(
    ownership_path: &Path,
    settings_path: &Path,
) -> Result<Option<IdeSettingOwnership>, HostIntegrationError> {
    let Some(ownership) = read_ownership_record_if_present(ownership_path)? else {
        return Ok(None);
    };
    if ownership.schema_version != OWNERSHIP_SCHEMA_VERSION
        || ownership.settings_path != settings_path
    {
        return Err(settings_conflict(
            "IDE setting ownership does not match the requested settings file",
        ));
    }
    Ok(Some(ownership))
}

pub(super) fn restore_after_failed_enable(
    ownership_path: &Path,
    previous: Option<&IdeSettingOwnership>,
) -> Result<(), HostIntegrationError> {
    match previous {
        Some(previous) => atomic_file::write_json_private(ownership_path, previous),
        None => atomic_file::remove_regular_file(ownership_path),
    }
}

fn read_ownership_record_if_present(
    ownership_path: &Path,
) -> Result<Option<IdeSettingOwnership>, HostIntegrationError> {
    if !ownership_path.exists() && !ownership_path.is_symlink() {
        return Ok(None);
    }
    let bytes = atomic_file::read_regular_file(ownership_path)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| HostIntegrationError::Json {
            path: ownership_path.to_path_buf(),
            source,
        })
}
