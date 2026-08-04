use super::{discovery, ownership, patch, CliIntegrationStatus};
use crate::error::{io_error, HostIntegrationError};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub(super) fn enable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    patch::validate_local_endpoint(target_endpoint)?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;

    let home = discovery::user_home_dir()
        .ok_or_else(|| HostIntegrationError::InvalidBundle("无法获取 Home 目录".to_string()))?;

    let target_files = discovery::target_shell_configs_for_write(&home);
    enable_cli_integration_after_preparation(integration_root, target_endpoint, &target_files)
}

#[cfg(test)]
pub(super) fn enable_cli_integration_with_target_files(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
    target_files: &[PathBuf],
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    patch::validate_local_endpoint(target_endpoint)?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;
    enable_cli_integration_after_preparation(integration_root, target_endpoint, target_files)
}

fn enable_cli_integration_after_preparation(
    integration_root: &Path,
    target_endpoint: &str,
    target_files: &[PathBuf],
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let mut transaction = FileTransaction::default();
    let mut updated_files = Vec::new();

    for file in target_files {
        if let Err(error) = transaction.snapshot(file) {
            return rollback_with_error(&transaction, error);
        }

        let is_fish = patch::is_fish_config(file);
        let snippet = patch::snippet_for(target_endpoint, is_fish);
        if let Err(error) = patch::update_shell_config_file(file, &snippet, is_fish) {
            return rollback_with_error(&transaction, error);
        }
        updated_files.push(file.clone());
    }

    let helper_env_path = integration_root.join("cli-integration").join("env.sh");
    if let Err(error) = transaction.snapshot(&helper_env_path) {
        return rollback_with_error(&transaction, error);
    }
    if let Some(parent) = helper_env_path.parent() {
        if let Err(error) = fs::create_dir_all(parent).map_err(|e| io_error(parent, e)) {
            return rollback_with_error(&transaction, error);
        }
    }
    let helper_content =
        format!("# AGY BYOK CLI Integration Helper\nexport CLOUD_CODE_URL=\"{target_endpoint}\"\n");
    if let Err(error) =
        fs::write(&helper_env_path, helper_content).map_err(|e| io_error(&helper_env_path, e))
    {
        return rollback_with_error(&transaction, error);
    }

    let ownership_path = integration_root.join(super::CLI_OWNERSHIP_FILE);
    if let Err(error) = transaction.snapshot(&ownership_path) {
        return rollback_with_error(&transaction, error);
    }
    if let Err(error) = ownership::write_ownership(integration_root, target_endpoint, updated_files)
    {
        return rollback_with_error(&transaction, error);
    }

    match super::discovery::inspect_cli_integration(integration_root, target_endpoint) {
        Ok(status) => Ok(status),
        Err(error) => rollback_with_error(&transaction, error),
    }
}

fn rollback_with_error<T>(
    transaction: &FileTransaction,
    error: HostIntegrationError,
) -> Result<T, HostIntegrationError> {
    transaction.rollback();
    Err(error)
}

#[derive(Default)]
struct FileTransaction {
    snapshots: Vec<FileSnapshot>,
}

impl FileTransaction {
    fn snapshot(&mut self, path: &Path) -> Result<(), HostIntegrationError> {
        if self
            .snapshots
            .iter()
            .any(|snapshot| snapshot.path.as_path() == path)
        {
            return Ok(());
        }
        self.snapshots.push(FileSnapshot::capture(path)?);
        Ok(())
    }

    fn rollback(&self) {
        for snapshot in self.snapshots.iter().rev() {
            let _ = snapshot.restore();
        }
    }
}

struct FileSnapshot {
    path: PathBuf,
    original: OriginalFile,
}

impl FileSnapshot {
    fn capture(path: &Path) -> Result<Self, HostIntegrationError> {
        let original = match fs::symlink_metadata(path) {
            Ok(_) if path.is_file() => {
                OriginalFile::Regular(fs::read(path).map_err(|e| io_error(path, e))?)
            }
            Ok(_) => OriginalFile::Other,
            Err(error) if error.kind() == ErrorKind::NotFound => OriginalFile::Missing,
            Err(error) => return Err(io_error(path, error)),
        };

        Ok(Self {
            path: path.to_path_buf(),
            original,
        })
    }

    fn restore(&self) -> Result<(), HostIntegrationError> {
        match &self.original {
            OriginalFile::Missing => match fs::symlink_metadata(&self.path) {
                Ok(metadata)
                    if metadata.file_type().is_file() || metadata.file_type().is_symlink() =>
                {
                    fs::remove_file(&self.path).map_err(|e| io_error(&self.path, e))
                }
                Ok(_) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(io_error(&self.path, error)),
            },
            OriginalFile::Regular(content) => {
                fs::write(&self.path, content).map_err(|e| io_error(&self.path, e))
            }
            OriginalFile::Other => Ok(()),
        }
    }
}

enum OriginalFile {
    Missing,
    Regular(Vec<u8>),
    Other,
}

pub(super) fn disable_cli_integration(
    integration_root: impl AsRef<Path>,
    target_endpoint: &str,
) -> Result<CliIntegrationStatus, HostIntegrationError> {
    let integration_root = integration_root.as_ref();
    let home = discovery::user_home_dir()
        .ok_or_else(|| HostIntegrationError::InvalidBundle("无法获取 Home 目录".to_string()))?;
    let current = discovery::inspect_cli_integration(integration_root, target_endpoint)?;
    let external_endpoint = (current.state == super::CliIntegrationState::Mismatch
        && !current.has_ownership)
        .then(|| current.configured_endpoint.clone())
        .flatten();

    let candidate_files = discovery::candidate_shell_configs(&home);
    for file in &candidate_files {
        if file.is_file() {
            let is_fish = patch::is_fish_config(file);
            patch::remove_snippet_from_file(file, is_fish)?;
            if let Some(endpoint) = external_endpoint.as_deref() {
                patch::remove_endpoint_assignment_from_file(file, is_fish, endpoint)?;
            }
        }
    }

    let ownership_path = integration_root.join(super::CLI_OWNERSHIP_FILE);
    if ownership_path.is_file() {
        let _ = fs::remove_file(&ownership_path);
    }

    super::discovery::inspect_cli_integration(integration_root, target_endpoint)
}

fn prepare_private_directory(path: &Path) -> Result<(), HostIntegrationError> {
    fs::create_dir_all(path).map_err(|e| io_error(path, e))?;
    Ok(())
}
