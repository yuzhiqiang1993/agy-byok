use crate::error::{io_error, HostIntegrationError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct CliOwnership {
    pub(super) schema_version: u32,
    pub(super) managed_endpoint: String,
    pub(super) updated_files: Vec<PathBuf>,
}

pub(super) fn read_ownership_if_present(
    ownership_path: &Path,
) -> Result<Option<CliOwnership>, HostIntegrationError> {
    if !ownership_path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(ownership_path).map_err(|e| io_error(ownership_path, e))?;
    let ownership: CliOwnership = serde_json::from_slice(&bytes).map_err(|e| {
        HostIntegrationError::InvalidBundle(format!("无法解析 CLI ownership 格式: {e}"))
    })?;
    if ownership.schema_version == super::OWNERSHIP_SCHEMA_VERSION {
        Ok(Some(ownership))
    } else {
        Ok(None)
    }
}

pub(super) fn write_ownership(
    integration_root: &Path,
    target_endpoint: &str,
    updated_files: Vec<PathBuf>,
) -> Result<(), HostIntegrationError> {
    let ownership = CliOwnership {
        schema_version: super::OWNERSHIP_SCHEMA_VERSION,
        managed_endpoint: target_endpoint.to_string(),
        updated_files,
    };

    let ownership_path = integration_root.join(super::CLI_OWNERSHIP_FILE);
    let ownership_bytes = serde_json::to_vec_pretty(&ownership).map_err(|e| {
        HostIntegrationError::InvalidBundle(format!("无法序列化 CLI ownership: {e}"))
    })?;
    fs::write(&ownership_path, ownership_bytes).map_err(|e| io_error(&ownership_path, e))
}
