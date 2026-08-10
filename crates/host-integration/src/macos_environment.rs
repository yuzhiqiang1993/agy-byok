//! macOS 用户会话环境变量接入。
//!
//! Antigravity App 与 CLI 都读取 `CLOUD_CODE_URL`。两者共享 ownership，避免一个入口
//! 停用时移除另一个入口仍在使用的会话变量。App 包本身保持厂商签名完整。

mod launchctl;
mod receipt;

#[cfg(test)]
mod tests;

use crate::error::HostIntegrationError;
use receipt::{
    prepare_private_directory, read_receipt_if_present, receipt_path, remove_receipt,
    restore_receipt_after_failed_enable, write_receipt, MacOsEnvironmentOwners,
    MacOsEnvironmentReceipt, RECEIPT_SCHEMA_VERSION,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsEnvironmentOwner {
    App,
    Cli,
}

#[derive(Debug, Clone)]
pub struct MacOsEnvironmentStatus {
    pub configured_endpoint: Option<String>,
    current_value_is_managed: bool,
    owners: MacOsEnvironmentOwners,
}

impl MacOsEnvironmentStatus {
    pub fn has_owner(&self, owner: MacOsEnvironmentOwner) -> bool {
        self.owners.contains(owner)
    }

    pub fn is_active_for(&self, owner: MacOsEnvironmentOwner) -> bool {
        self.current_value_is_managed && self.has_owner(owner)
    }
}

pub fn inspect(
    integration_root: impl AsRef<Path>,
) -> Result<MacOsEnvironmentStatus, HostIntegrationError> {
    let receipt = read_receipt_if_present(&receipt_path(integration_root.as_ref()))?;
    let current_endpoint = launchctl::read_endpoint()?;
    Ok(status_from(receipt, current_endpoint))
}

fn status_from(
    receipt: Option<MacOsEnvironmentReceipt>,
    current_endpoint: Option<String>,
) -> MacOsEnvironmentStatus {
    let current_value_is_managed = receipt.as_ref().is_some_and(|receipt| {
        current_endpoint.as_deref() == Some(receipt.managed_endpoint.as_str())
    });

    MacOsEnvironmentStatus {
        configured_endpoint: current_endpoint,
        current_value_is_managed,
        owners: receipt.map_or_else(MacOsEnvironmentOwners::empty, |receipt| receipt.owners),
    }
}

pub fn enable(
    integration_root: impl AsRef<Path>,
    owner: MacOsEnvironmentOwner,
    target_endpoint: &str,
) -> Result<MacOsEnvironmentStatus, HostIntegrationError> {
    crate::local_endpoint::validate_local_endpoint(target_endpoint, "macOS 环境变量")?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;

    let receipt_file = receipt_path(integration_root);
    let previous_receipt = read_receipt_if_present(&receipt_file)?;
    let current_endpoint = launchctl::read_endpoint()?;
    let mut receipt = match previous_receipt.as_ref() {
        Some(receipt) if current_endpoint.as_deref() == Some(receipt.managed_endpoint.as_str()) => {
            let mut receipt = receipt.clone();
            receipt.managed_endpoint = target_endpoint.to_string();
            receipt
        }
        Some(receipt) => MacOsEnvironmentReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_endpoint: target_endpoint.to_string(),
            original_endpoint: current_endpoint,
            owners: receipt.owners.clone(),
        },
        None => MacOsEnvironmentReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_endpoint: target_endpoint.to_string(),
            original_endpoint: current_endpoint,
            owners: MacOsEnvironmentOwners::with(owner),
        },
    };
    receipt.owners.insert(owner);

    write_receipt(&receipt_file, &receipt)?;
    if let Err(environment_error) = launchctl::set_endpoint(target_endpoint) {
        if let Err(recovery_error) =
            restore_receipt_after_failed_enable(&receipt_file, previous_receipt.as_ref())
        {
            return Err(HostIntegrationError::RecoveryFailed {
                operation: environment_error.to_string(),
                recovery: recovery_error.to_string(),
            });
        }
        return Err(environment_error);
    }
    Ok(MacOsEnvironmentStatus {
        configured_endpoint: Some(target_endpoint.to_string()),
        current_value_is_managed: true,
        owners: receipt.owners,
    })
}

pub fn disable(
    integration_root: impl AsRef<Path>,
    owner: MacOsEnvironmentOwner,
) -> Result<MacOsEnvironmentStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let receipt_file = receipt_path(integration_root);
    let receipt = read_receipt_if_present(&receipt_file)?;

    if let Some(receipt) = receipt {
        let current_endpoint = launchctl::read_endpoint()?;
        let current_value_is_managed =
            current_endpoint.as_deref() == Some(receipt.managed_endpoint.as_str());
        let mut remaining_receipt = receipt.clone();
        remaining_receipt.owners.remove(owner);

        if !remaining_receipt.owners.is_empty() {
            write_receipt(&receipt_file, &remaining_receipt)?;
            return Ok(MacOsEnvironmentStatus {
                configured_endpoint: current_endpoint,
                current_value_is_managed,
                owners: remaining_receipt.owners,
            });
        }

        remove_receipt(&receipt_file)?;
        if current_value_is_managed {
            let restore_result = match receipt.original_endpoint.as_deref() {
                Some(original_endpoint) => launchctl::set_endpoint(original_endpoint),
                None => launchctl::remove_endpoint(),
            };
            if let Err(environment_error) = restore_result {
                if let Err(recovery_error) = write_receipt(&receipt_file, &receipt) {
                    return Err(HostIntegrationError::RecoveryFailed {
                        operation: environment_error.to_string(),
                        recovery: recovery_error.to_string(),
                    });
                }
                return Err(environment_error);
            }
        } else {
            launchctl::remove_endpoint()?;
        }
        return Ok(MacOsEnvironmentStatus {
            configured_endpoint: if current_value_is_managed {
                receipt.original_endpoint
            } else {
                None
            },
            current_value_is_managed: false,
            owners: MacOsEnvironmentOwners::empty(),
        });
    }

    if launchctl::read_endpoint()?.is_some() {
        launchctl::remove_endpoint()?;
    }
    Ok(MacOsEnvironmentStatus {
        configured_endpoint: None,
        current_value_is_managed: false,
        owners: MacOsEnvironmentOwners::empty(),
    })
}

/// 登录会话重建后，恢复用户已显式启用的环境变量；外部非空值始终优先。
pub fn reconcile(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<MacOsEnvironmentStatus, HostIntegrationError> {
    crate::local_endpoint::validate_local_endpoint(target_endpoint, "macOS 环境变量")?;
    let integration_root = integration_root.as_ref();
    let receipt_file = receipt_path(integration_root);
    let receipt = read_receipt_if_present(&receipt_file)?;
    let current_endpoint = launchctl::read_endpoint()?;

    if let Some(previous_receipt) = receipt.as_ref() {
        let session_was_reset = current_endpoint.is_none();
        let endpoint_changed_by_agy = previous_receipt.managed_endpoint != target_endpoint
            && current_endpoint.as_deref() == Some(previous_receipt.managed_endpoint.as_str());
        if session_was_reset || endpoint_changed_by_agy {
            let mut updated_receipt = previous_receipt.clone();
            updated_receipt.managed_endpoint = target_endpoint.to_string();
            write_receipt(&receipt_file, &updated_receipt)?;
            if let Err(environment_error) = launchctl::set_endpoint(target_endpoint) {
                if let Err(recovery_error) = write_receipt(&receipt_file, previous_receipt) {
                    return Err(HostIntegrationError::RecoveryFailed {
                        operation: environment_error.to_string(),
                        recovery: recovery_error.to_string(),
                    });
                }
                return Err(environment_error);
            }
            return Ok(status_from(
                Some(updated_receipt),
                Some(target_endpoint.to_string()),
            ));
        }
    }

    Ok(status_from(receipt, current_endpoint))
}
