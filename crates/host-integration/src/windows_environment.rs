//! Windows 用户级环境变量接入。
//!
//! Antigravity App 与 CLI 都通过 `CLOUD_CODE_URL` 读取本地代理地址，因此必须共享同一份
//! ownership receipt，避免一个入口停用时误删另一个入口仍在使用的配置。

mod receipt;
mod registry;

use crate::error::HostIntegrationError;
use receipt::{
    prepare_private_directory, read_receipt_if_present, receipt_path, remove_receipt,
    restore_receipt_after_failed_enable, write_receipt, WindowsEnvironmentOwners,
    WindowsEnvironmentReceipt, RECEIPT_SCHEMA_VERSION,
};
use registry::{
    delete_user_environment_value, read_user_environment_value, write_user_environment_value,
    RegistryStringKind, RegistryStringValue,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsEnvironmentOwner {
    App,
    Cli,
}

#[derive(Debug, Clone)]
pub struct WindowsEnvironmentStatus {
    pub configured_endpoint: Option<String>,
    current_value_is_managed: bool,
    owners: WindowsEnvironmentOwners,
}

impl WindowsEnvironmentStatus {
    /// receipt 是否记录了指定入口。
    pub fn has_owner(&self, owner: WindowsEnvironmentOwner) -> bool {
        self.owners.contains(owner)
    }

    /// 指定入口是否仍安全地持有当前 Windows 用户级变量。
    pub fn is_active_for(&self, owner: WindowsEnvironmentOwner) -> bool {
        self.current_value_is_managed && self.has_owner(owner)
    }
}

pub fn inspect(
    integration_root: impl AsRef<Path>,
) -> Result<WindowsEnvironmentStatus, HostIntegrationError> {
    let receipt = read_receipt_if_present(&receipt_path(integration_root.as_ref()))?;
    let current_value = read_user_environment_value()?;
    let current_value_is_managed = receipt
        .as_ref()
        .is_some_and(|receipt| receipt_matches_current_value(receipt, current_value.as_ref()));

    Ok(WindowsEnvironmentStatus {
        configured_endpoint: current_value.as_ref().map(|value| value.value.clone()),
        current_value_is_managed,
        owners: receipt.map_or_else(WindowsEnvironmentOwners::empty, |receipt| receipt.owners),
    })
}

pub fn enable(
    integration_root: impl AsRef<Path>,
    owner: WindowsEnvironmentOwner,
    target_endpoint: &str,
) -> Result<WindowsEnvironmentStatus, HostIntegrationError> {
    crate::local_endpoint::validate_local_endpoint(target_endpoint, "Windows 环境变量")?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;

    let receipt_file = receipt_path(integration_root);
    let previous_receipt = read_receipt_if_present(&receipt_file)?;
    let current_value = read_user_environment_value()?;
    let mut receipt = match previous_receipt.as_ref() {
        Some(receipt) if receipt_matches_current_value(receipt, current_value.as_ref()) => {
            let mut receipt = receipt.clone();
            receipt.managed_endpoint = target_endpoint.to_string();
            receipt.owners.insert(owner);
            receipt
        }
        Some(receipt) => WindowsEnvironmentReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_endpoint: target_endpoint.to_string(),
            // 用户已手动接管当前值时，显式再次启用应以该值作为新的恢复基线。
            original_cloud_code_url: current_value,
            owners: receipt.owners.clone(),
        },
        None => WindowsEnvironmentReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_endpoint: target_endpoint.to_string(),
            original_cloud_code_url: current_value,
            owners: WindowsEnvironmentOwners {
                app: owner == WindowsEnvironmentOwner::App,
                cli: owner == WindowsEnvironmentOwner::Cli,
            },
        },
    };

    // 更新入口时保留首次接入前的原值，最后一个入口停用后才能无损恢复。
    receipt.owners.insert(owner);
    write_receipt(&receipt_file, &receipt)?;
    if let Err(error) = write_user_environment_value(&RegistryStringValue {
        value: target_endpoint.to_string(),
        kind: RegistryStringKind::String,
    }) {
        if let Err(recovery_error) =
            restore_receipt_after_failed_enable(&receipt_file, previous_receipt.as_ref())
        {
            return Err(HostIntegrationError::RecoveryFailed {
                operation: error.to_string(),
                recovery: recovery_error.to_string(),
            });
        }
        return Err(error);
    }
    Ok(WindowsEnvironmentStatus {
        configured_endpoint: Some(target_endpoint.to_string()),
        current_value_is_managed: true,
        owners: receipt.owners,
    })
}

pub fn disable(
    integration_root: impl AsRef<Path>,
    owner: WindowsEnvironmentOwner,
) -> Result<WindowsEnvironmentStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let receipt_file = receipt_path(integration_root);
    let receipt = read_receipt_if_present(&receipt_file)?;

    if let Some(receipt) = receipt {
        let current_value = read_user_environment_value()?;
        let current_value_is_managed = receipt_matches_current_value(&receipt, current_value.as_ref());
        let mut remaining_receipt = receipt.clone();
        remaining_receipt.owners.remove(owner);

        if !remaining_receipt.owners.is_empty() {
            write_receipt(&receipt_file, &remaining_receipt)?;
            return Ok(WindowsEnvironmentStatus {
                configured_endpoint: current_value.as_ref().map(|value| value.value.clone()),
                current_value_is_managed,
                owners: remaining_receipt.owners,
            });
        }

        remove_receipt(&receipt_file)?;
        if current_value_is_managed {
            let restore_result = match receipt.original_cloud_code_url.as_ref() {
                Some(original_value) => write_user_environment_value(original_value),
                None => delete_user_environment_value(),
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
            delete_user_environment_value()?;
        }
        return Ok(WindowsEnvironmentStatus {
            configured_endpoint: if current_value_is_managed {
                receipt.original_cloud_code_url.map(|value| value.value)
            } else {
                None
            },
            current_value_is_managed: false,
            owners: WindowsEnvironmentOwners::empty(),
        });
    }

    if read_user_environment_value()?.is_some() {
        delete_user_environment_value()?;
    }
    Ok(WindowsEnvironmentStatus {
        configured_endpoint: None,
        current_value_is_managed: false,
        owners: WindowsEnvironmentOwners::empty(),
    })
}

fn receipt_matches_current_value(
    receipt: &WindowsEnvironmentReceipt,
    current_value: Option<&RegistryStringValue>,
) -> bool {
    matches!(
        current_value,
        Some(value)
            if value.kind == RegistryStringKind::String
                && value.value == receipt.managed_endpoint
    )
}

#[cfg(test)]
mod tests;
