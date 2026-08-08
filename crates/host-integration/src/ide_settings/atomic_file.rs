use super::settings_conflict;
use crate::atomic_file as shared;
use crate::error::{io_error, HostIntegrationError};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn validate_settings_path(path: &Path) -> Result<PathBuf, HostIntegrationError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
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
    shared::write_preserving_permissions(path, bytes)
}

pub(super) fn write_json_private<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), HostIntegrationError> {
    shared::write_json_private(path, value)
}

pub(super) fn remove_regular_file(path: &Path) -> Result<(), HostIntegrationError> {
    shared::remove_regular_file(path)
}
