use crate::error::{io_error, HostIntegrationError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct AppIntegrationReceipt {
    pub(super) schema_version: u32,
    pub(super) app_path: String,
    pub(super) app_version: Option<String>,
    pub(super) original_sha256: String,
    pub(super) endpoint: String,
    pub(super) wrapper_sha256: String,
}

pub(super) fn read_receipt(path: &Path) -> Result<AppIntegrationReceipt, HostIntegrationError> {
    let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
    serde_json::from_slice(&bytes).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn read_receipt_required(
    path: &Path,
) -> Result<AppIntegrationReceipt, HostIntegrationError> {
    read_receipt(path).map_err(|error| {
        HostIntegrationError::AppIntegrationConflict(format!(
            "无法读取接入凭据 {}：{error}",
            path.display()
        ))
    })
}

pub(super) fn write_receipt(
    path: &Path,
    receipt: &AppIntegrationReceipt,
) -> Result<(), HostIntegrationError> {
    let bytes =
        serde_json::to_vec_pretty(receipt).map_err(|source| HostIntegrationError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    super::transaction::write_atomic(path, &bytes, 0o600)
}
