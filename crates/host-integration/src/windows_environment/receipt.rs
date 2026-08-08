use super::registry::RegistryStringValue;
use super::WindowsEnvironmentOwner;
use crate::atomic_file;
use crate::error::{io_error, HostIntegrationError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const RECEIPT_FILE: &str = "windows-environment.json";
pub(super) const RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowsEnvironmentOwners {
    pub(super) app: bool,
    pub(super) cli: bool,
}

impl WindowsEnvironmentOwners {
    pub(super) const fn empty() -> Self {
        Self {
            app: false,
            cli: false,
        }
    }

    pub(super) fn contains(&self, owner: WindowsEnvironmentOwner) -> bool {
        match owner {
            WindowsEnvironmentOwner::App => self.app,
            WindowsEnvironmentOwner::Cli => self.cli,
        }
    }

    pub(super) fn insert(&mut self, owner: WindowsEnvironmentOwner) {
        match owner {
            WindowsEnvironmentOwner::App => self.app = true,
            WindowsEnvironmentOwner::Cli => self.cli = true,
        }
    }

    pub(super) fn remove(&mut self, owner: WindowsEnvironmentOwner) {
        match owner {
            WindowsEnvironmentOwner::App => self.app = false,
            WindowsEnvironmentOwner::Cli => self.cli = false,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        !self.app && !self.cli
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowsEnvironmentReceipt {
    pub(super) schema_version: u32,
    pub(super) managed_endpoint: String,
    #[serde(deserialize_with = "crate::serde_helpers::required_nullable")]
    pub(super) original_cloud_code_url: Option<RegistryStringValue>,
    pub(super) owners: WindowsEnvironmentOwners,
}

pub(super) fn receipt_path(integration_root: &Path) -> PathBuf {
    integration_root.join(RECEIPT_FILE)
}

pub(super) fn prepare_private_directory(path: &Path) -> Result<(), HostIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(HostIntegrationError::InvalidIntegration(
                "Windows 环境变量接入目录必须是常规目录".to_string(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    fs::create_dir_all(path).map_err(|error| io_error(path, error))
}

pub(super) fn read_receipt_if_present(
    receipt_file: &Path,
) -> Result<Option<WindowsEnvironmentReceipt>, HostIntegrationError> {
    let metadata = match fs::symlink_metadata(receipt_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(receipt_file, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(HostIntegrationError::InvalidIntegration(
            "Windows 环境变量 receipt 必须是常规文件".to_string(),
        ));
    }

    let bytes = fs::read(receipt_file).map_err(|error| io_error(receipt_file, error))?;
    let receipt: WindowsEnvironmentReceipt = serde_json::from_slice(&bytes).map_err(|error| {
        HostIntegrationError::InvalidIntegration(format!(
            "无法解析 Windows 环境变量 receipt: {error}"
        ))
    })?;
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION {
        return Err(HostIntegrationError::InvalidIntegration(
            "Windows 环境变量 receipt 版本不受支持".to_string(),
        ));
    }
    if receipt.owners.is_empty()
        || !crate::local_endpoint::is_local_proxy_endpoint(&receipt.managed_endpoint)
    {
        return Err(HostIntegrationError::InvalidIntegration(
            "Windows 环境变量 receipt 状态无效".to_string(),
        ));
    }
    Ok(Some(receipt))
}

pub(super) fn write_receipt(
    receipt_file: &Path,
    receipt: &WindowsEnvironmentReceipt,
) -> Result<(), HostIntegrationError> {
    atomic_file::write_json_private(receipt_file, receipt)
}

pub(super) fn restore_receipt_after_failed_enable(
    receipt_file: &Path,
    previous_receipt: Option<&WindowsEnvironmentReceipt>,
) -> Result<(), HostIntegrationError> {
    match previous_receipt {
        Some(receipt) => write_receipt(receipt_file, receipt),
        None => remove_receipt(receipt_file),
    }
}

pub(super) fn remove_receipt(receipt_file: &Path) -> Result<(), HostIntegrationError> {
    let metadata = match fs::symlink_metadata(receipt_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(receipt_file, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(HostIntegrationError::InvalidIntegration(
            "Windows 环境变量 receipt 必须是常规文件".to_string(),
        ));
    }
    atomic_file::remove_regular_file(receipt_file)
}
