use super::{discovery, ownership, patch, AppIntegrationState, AppIntegrationStatus};
use crate::error::{io_error, HostIntegrationError};
use crate::sha256;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn enable_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    patch::validate_local_endpoint(endpoint)?;
    let paths = discovery::AppIntegrationPaths::new(app_path);
    discovery::ensure_bundle_directories(&paths)?;
    let current = discovery::inspect_app_integration(&paths.app_path, endpoint)?;
    if current.state == AppIntegrationState::Conflict {
        return Err(HostIntegrationError::AppIntegrationConflict(
            current.message,
        ));
    }

    let app_version = discovery::read_app_version(&paths.app_path)?;
    let (original_sha256, had_existing_wrapper, previous_wrapper, previous_receipt) =
        if current.state == AppIntegrationState::Disabled {
            discovery::ensure_regular_file(&paths.wrapper_path, "language_server")?;
            let original_bytes = discovery::read_file(&paths.wrapper_path)?;
            if patch::is_managed_wrapper_bytes(&original_bytes) {
                return Err(HostIntegrationError::AppIntegrationConflict(
                    "language_server 看起来已经是 Wrapper，但缺少可验证的原始二进制".to_string(),
                ));
            }
            (sha256(&original_bytes), false, None, None)
        } else {
            discovery::ensure_regular_file(&paths.wrapper_path, "language_server")?;
            discovery::ensure_regular_file(&paths.real_bin_path, "language_server.real")?;
            let wrapper = discovery::read_file(&paths.wrapper_path)?;
            let receipt = if discovery::path_exists(&paths.receipt_path)? {
                Some(ownership::read_receipt_required(&paths.receipt_path)?)
            } else {
                None
            };
            let original_sha256 = match receipt.as_ref() {
                Some(item) => item.original_sha256.clone(),
                None => sha256(&discovery::read_file(&paths.real_bin_path)?),
            };
            (original_sha256, true, Some(wrapper), receipt)
        };

    if !had_existing_wrapper {
        fs::rename(&paths.wrapper_path, &paths.real_bin_path)
            .map_err(|error| io_error(&paths.wrapper_path, error))?;
    }

    let wrapper = patch::wrapper_script(endpoint);
    if let Err(error) = write_atomic(&paths.wrapper_path, wrapper.as_bytes(), 0o755) {
        if !had_existing_wrapper {
            let _ = fs::rename(&paths.real_bin_path, &paths.wrapper_path);
        }
        return Err(error);
    }

    let receipt = ownership::AppIntegrationReceipt {
        schema_version: super::APP_INTEGRATION_SCHEMA_VERSION,
        app_path: paths.app_path.display().to_string(),
        app_version,
        original_sha256,
        endpoint: endpoint.to_string(),
        wrapper_sha256: sha256(wrapper.as_bytes()),
    };
    if let Err(error) = ownership::write_receipt(&paths.receipt_path, &receipt) {
        if let Some(previous_wrapper) = previous_wrapper {
            let _ = write_atomic(&paths.wrapper_path, &previous_wrapper, 0o755);
        } else {
            let _ = fs::remove_file(&paths.wrapper_path);
            let _ = fs::rename(&paths.real_bin_path, &paths.wrapper_path);
        }
        if let Some(previous_receipt) = previous_receipt {
            let _ = ownership::write_receipt(&paths.receipt_path, &previous_receipt);
        } else {
            let _ = fs::remove_file(&paths.receipt_path);
        }
        return Err(error);
    }

    discovery::inspect_app_integration(&paths.app_path, endpoint)
}

pub(super) fn disable_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    patch::validate_local_endpoint(endpoint)?;
    let paths = discovery::AppIntegrationPaths::new(app_path);
    discovery::ensure_bundle_directories(&paths)?;
    let current = discovery::inspect_app_integration(&paths.app_path, endpoint)?;
    match current.state {
        AppIntegrationState::Disabled => return Ok(current),
        AppIntegrationState::Conflict => {
            return Err(HostIntegrationError::AppIntegrationConflict(
                current.message,
            ));
        }
        AppIntegrationState::Managed | AppIntegrationState::Mismatch => {}
    }

    discovery::ensure_regular_file(&paths.wrapper_path, "language_server")?;
    discovery::ensure_regular_file(&paths.real_bin_path, "language_server.real")?;
    let receipt_exists = discovery::path_exists(&paths.receipt_path)?;
    if receipt_exists {
        discovery::ensure_regular_file(&paths.receipt_path, "接入凭据")?;
    }

    let backup_path = temporary_path(&paths.wrapper_path);
    fs::rename(&paths.wrapper_path, &backup_path)
        .map_err(|error| io_error(&paths.wrapper_path, error))?;
    if let Err(error) = fs::rename(&paths.real_bin_path, &paths.wrapper_path) {
        let _ = fs::rename(&backup_path, &paths.wrapper_path);
        return Err(io_error(&paths.real_bin_path, error));
    }

    if receipt_exists {
        if let Err(error) = fs::remove_file(&paths.receipt_path) {
            let _ = fs::rename(&paths.wrapper_path, &paths.real_bin_path);
            let _ = fs::rename(&backup_path, &paths.wrapper_path);
            return Err(io_error(&paths.receipt_path, error));
        }
    }
    fs::remove_file(&backup_path).map_err(|error| io_error(&backup_path, error))?;

    discovery::inspect_app_integration(&paths.app_path, endpoint)
}

pub(super) fn write_atomic(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<(), HostIntegrationError> {
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(bytes)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
            .map_err(|error| io_error(&temporary, error))?;
        fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("language_server");
    path.with_file_name(format!(
        ".{file_name}.tmp-{}-{timestamp}",
        std::process::id()
    ))
}
