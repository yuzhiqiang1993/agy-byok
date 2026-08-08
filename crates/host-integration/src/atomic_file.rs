use crate::error::{io_error, HostIntegrationError};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn write_preserving_permissions(
    path: &Path,
    bytes: &[u8],
) -> Result<(), HostIntegrationError> {
    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(HostIntegrationError::InvalidIntegration(format!(
                "写入目标必须是常规文件：{}",
                path.display()
            )));
        }
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(path, error)),
    };
    atomic_write(path, bytes, permissions.as_ref())
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    atomic_write(path, bytes, None)
}

pub(crate) fn write_json_private<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), HostIntegrationError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    write_private(path, &bytes)
}

pub(crate) fn remove_regular_file(path: &Path) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HostIntegrationError::InvalidIntegration(format!(
            "删除目标必须是常规文件：{}",
            path.display()
        )));
    }
    fs::remove_file(path).map_err(|error| io_error(path, error))?;
    if let Some(parent) = path.parent() {
        sync_directory_after_commit(parent, path);
    }
    Ok(())
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<(), HostIntegrationError> {
    let parent = path.parent().ok_or_else(|| {
        HostIntegrationError::InvalidIntegration(format!("写入目标缺少父目录：{}", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    if path.is_symlink() {
        return Err(HostIntegrationError::InvalidIntegration(format!(
            "写入目标不能是符号链接：{}",
            path.display()
        )));
    }

    let temporary = parent.join(format!(
        ".agy-byok-atomic-{}-{}.next",
        std::process::id(),
        unix_time_ns()
    ));
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
        replace_file(&temporary, path)?;
        sync_directory_after_commit(parent, path);
        Ok(())
    })();
    if result.is_err() {
        remove_temporary_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), HostIntegrationError> {
    let directory = fs::File::open(path).map_err(|error| io_error(path, error))?;
    directory.sync_all().map_err(|error| io_error(path, error))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), HostIntegrationError> {
    Ok(())
}

fn sync_directory_after_commit(parent: &Path, committed_path: &Path) {
    if let Err(error) = sync_directory(parent) {
        tracing::warn!(
            path = %committed_path.display(),
            %error,
            "宿主接入文件已提交，但父目录同步失败"
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
    fs::rename(source, destination).map_err(|error| io_error(destination, error))
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved != 0 {
        Ok(())
    } else {
        Err(io_error(destination, std::io::Error::last_os_error()))
    }
}

fn remove_temporary_file(path: &Path) {
    if path.is_file() && !path.is_symlink() {
        let _ = fs::remove_file(path);
    }
}

fn unix_time_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_complete_content() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, b"before").unwrap();

        write_preserving_permissions(&path, b"after").unwrap();

        assert_eq!(fs::read(path).unwrap(), b"after");
    }

    #[cfg(unix)]
    #[test]
    fn preserving_write_keeps_mode_and_rejects_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::write(&target, b"before").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

        write_preserving_permissions(&target, b"after").unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );

        symlink(&target, &link).unwrap();
        assert!(write_preserving_permissions(&link, b"unexpected").is_err());
        assert_eq!(fs::read(target).unwrap(), b"after");
    }
}
