use super::settings_conflict;
use crate::error::{io_error, HostIntegrationError};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn validate_settings_path(path: &Path) -> Result<PathBuf, HostIntegrationError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(settings_conflict(
            "IDE settings path must be an absolute path without parent traversal",
        ));
    }
    if path.is_symlink() {
        return Err(settings_conflict("IDE settings path must not be a symlink"));
    }
    Ok(path.to_path_buf())
}

pub(super) fn validate_integration_root_if_present(
    path: &Path,
) -> Result<(), HostIntegrationError> {
    if path.exists() || path.is_symlink() {
        let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(settings_conflict(
                "IDE settings integration root must be a regular directory",
            ));
        }
    }
    Ok(())
}

pub(super) fn prepare_private_directory(path: &Path) -> Result<(), HostIntegrationError> {
    validate_integration_root_if_present(path)?;
    fs::create_dir_all(path).map_err(|error| io_error(path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error(path, error))?;
    }
    Ok(())
}

pub(super) fn read_optional_regular_file(
    path: &Path,
) -> Result<Option<Vec<u8>>, HostIntegrationError> {
    if !path.exists() && !path.is_symlink() {
        return Ok(None);
    }
    read_regular_file(path).map(Some)
}

pub(super) fn read_regular_file(path: &Path) -> Result<Vec<u8>, HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(settings_conflict(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    fs::read(path).map_err(|error| io_error(path, error))
}

pub(super) fn write_settings_file(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| settings_conflict("IDE settings path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    if path.is_symlink() {
        return Err(settings_conflict("IDE settings path must not be a symlink"));
    }
    let permissions = if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(settings_conflict(
                "IDE settings path must be a regular file",
            ));
        }
        Some(metadata.permissions())
    } else {
        None
    };
    atomic_write(path, bytes, permissions.as_ref())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    atomic_write(path, bytes, None)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(path, error))?;
    }
    Ok(())
}

pub(super) fn write_json_private<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), HostIntegrationError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    write_private_file(path, &bytes)
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<(), HostIntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| settings_conflict("target path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temporary = parent.join(format!(
        ".agy-byok-settings-{}-{}.next",
        std::process::id(),
        unix_time_ms()
    ));
    if temporary.exists() || temporary.is_symlink() {
        return Err(settings_conflict(
            "temporary IDE settings path already exists",
        ));
    }
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(bytes)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&temporary, permissions.clone())
                .map_err(|error| io_error(&temporary, error))?;
        }
        fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        remove_file_if_present(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), HostIntegrationError> {
    let directory = fs::File::open(path).map_err(|error| io_error(path, error))?;
    directory.sync_all().map_err(|error| io_error(path, error))
}

pub(super) fn remove_regular_file(path: &Path) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(settings_conflict(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    fs::remove_file(path).map_err(|error| io_error(path, error))?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) {
    if path.is_file() && !path.is_symlink() {
        let _ = fs::remove_file(path);
    }
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
