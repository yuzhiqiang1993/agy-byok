use crate::discovery::{discover, HostInstallation};
use crate::error::{io_error, HostIntegrationError};
use crate::profile::{safe_join, PatchProfile};
use crate::signing::CodeSignatureVerifier;

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_BUNDLE_DIRECTORY: &str = "original.app";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchTransactionState {
    Prepared,
    Applied,
    Restored,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BundleSnapshotStrategy {
    ClonePreferredCopyFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchReceipt {
    pub schema_version: u32,
    pub state: PatchTransactionState,
    pub profile_id: String,
    pub app_path: PathBuf,
    pub bundle_id: String,
    pub app_version: String,
    pub extension_version: String,
    pub extension_relative_path: PathBuf,
    pub original_sha256: String,
    pub patched_sha256: String,
    pub executable_relative_path: PathBuf,
    pub executable_sha256: String,
    pub endpoint: String,
    pub snapshot_bundle_relative_path: PathBuf,
    pub snapshot_strategy: BundleSnapshotStrategy,
    pub prepared_at_unix_ms: u128,
    pub applied_at_unix_ms: Option<u128>,
    pub restored_at_unix_ms: Option<u128>,
}

pub fn dry_run(
    app_path: impl AsRef<Path>,
    profile: &PatchProfile,
) -> Result<String, HostIntegrationError> {
    let installation = discover(app_path, &profile.layout)?;
    profile.validate_for_apply(&installation)?;
    let extension_path = safe_join(&installation.app_path, &profile.layout.extension_entry)?;
    let source =
        fs::read_to_string(&extension_path).map_err(|source| io_error(&extension_path, source))?;
    profile.create_candidate(&source)
}

pub fn restore(
    app_path: impl AsRef<Path>,
    profile: &PatchProfile,
    receipt_path: impl AsRef<Path>,
    verifier: &dyn CodeSignatureVerifier,
) -> Result<PatchReceipt, HostIntegrationError> {
    let app_path = app_path.as_ref();
    let receipt_path = receipt_path.as_ref();
    let receipt_bytes = fs::read(receipt_path).map_err(|source| io_error(receipt_path, source))?;
    let mut receipt: PatchReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|source| HostIntegrationError::Json {
            path: receipt_path.to_path_buf(),
            source,
        })?;
    validate_receipt(app_path, profile, receipt_path, &receipt)?;
    if receipt.state != PatchTransactionState::Applied {
        return Err(HostIntegrationError::ReceiptMismatch);
    }

    let current = discover(app_path, &profile.layout)?;
    validate_current_for_restore(&current, &receipt)?;

    let snapshot_path = resolve_snapshot_bundle_path(receipt_path, &receipt)?;
    let snapshot = discover(&snapshot_path, &profile.layout)?;
    validate_snapshot_against_receipt(&snapshot, &receipt)?;
    verifier.verify_vendor(&snapshot.app_path, &profile.bundle_id)?;

    let snapshot_extension_path = safe_join(&snapshot.app_path, &receipt.extension_relative_path)?;
    let original_bytes = fs::read(&snapshot_extension_path)
        .map_err(|source| io_error(&snapshot_extension_path, source))?;
    let extension_path = safe_join(app_path, &receipt.extension_relative_path)?;

    let restore_result = (|| {
        write_file_in_place(&extension_path, &original_bytes)?;
        let restored = discover(app_path, &profile.layout)?;
        if restored.extension_sha256 != receipt.original_sha256 {
            return Err(HostIntegrationError::HashMismatch {
                expected: receipt.original_sha256.clone(),
                actual: restored.extension_sha256,
            });
        }
        if restored.executable_relative_path != receipt.executable_relative_path
            || restored.executable_sha256 != receipt.executable_sha256
        {
            return Err(HostIntegrationError::HashMismatch {
                expected: receipt.executable_sha256.clone(),
                actual: restored.executable_sha256,
            });
        }
        verifier.verify_vendor(&restored.app_path, &profile.bundle_id)
    })();

    if let Err(restore_error) = restore_result {
        receipt.state = PatchTransactionState::RecoveryRequired;
        if let Err(receipt_error) = write_receipt(receipt_path, &receipt) {
            return Err(HostIntegrationError::CommandFailed(format!(
                "restore failed: {restore_error}; failed to persist recovery state: {receipt_error}"
            )));
        }
        return Err(restore_error);
    }

    receipt.state = PatchTransactionState::Restored;
    receipt.restored_at_unix_ms = Some(unix_time_ms());
    write_receipt(receipt_path, &receipt)?;
    Ok(receipt)
}

fn validate_current_for_restore(
    current: &HostInstallation,
    receipt: &PatchReceipt,
) -> Result<(), HostIntegrationError> {
    if current.bundle_id != receipt.bundle_id
        || current.app_version != receipt.app_version
        || current.extension_version != receipt.extension_version
        || current.executable_relative_path != receipt.executable_relative_path
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    if current.extension_sha256 != receipt.patched_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.patched_sha256.clone(),
            actual: current.extension_sha256.clone(),
        });
    }
    if current.executable_sha256 != receipt.executable_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.executable_sha256.clone(),
            actual: current.executable_sha256.clone(),
        });
    }
    Ok(())
}

fn validate_snapshot_against_receipt(
    snapshot: &HostInstallation,
    receipt: &PatchReceipt,
) -> Result<(), HostIntegrationError> {
    if snapshot.bundle_id != receipt.bundle_id
        || snapshot.app_version != receipt.app_version
        || snapshot.extension_version != receipt.extension_version
        || snapshot.executable_relative_path != receipt.executable_relative_path
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    if snapshot.extension_sha256 != receipt.original_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.original_sha256.clone(),
            actual: snapshot.extension_sha256.clone(),
        });
    }
    if snapshot.executable_sha256 != receipt.executable_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.executable_sha256.clone(),
            actual: snapshot.executable_sha256.clone(),
        });
    }
    Ok(())
}

fn validate_receipt(
    app_path: &Path,
    profile: &PatchProfile,
    receipt_path: &Path,
    receipt: &PatchReceipt,
) -> Result<(), HostIntegrationError> {
    let requested_app = canonicalize(app_path)?;
    let receipt_app = canonicalize(&receipt.app_path)?;
    if receipt.schema_version != 2
        || receipt.profile_id != profile.id
        || requested_app != receipt_app
        || receipt.bundle_id != profile.bundle_id
        || receipt.app_version != profile.app_version
        || receipt.extension_version != profile.extension_version
        || receipt.extension_relative_path != profile.layout.extension_entry
        || receipt.original_sha256 != profile.original_sha256
        || receipt.patched_sha256 != profile.patched_sha256
        || receipt.endpoint != profile.endpoint
        || receipt.snapshot_bundle_relative_path != Path::new(SNAPSHOT_BUNDLE_DIRECTORY)
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    resolve_snapshot_bundle_path(receipt_path, receipt).map(|_| ())
}

fn resolve_snapshot_bundle_path(
    receipt_path: &Path,
    receipt: &PatchReceipt,
) -> Result<PathBuf, HostIntegrationError> {
    let receipt_directory = receipt_path
        .parent()
        .ok_or(HostIntegrationError::ReceiptMismatch)?;
    let joined = safe_join(receipt_directory, &receipt.snapshot_bundle_relative_path)?;
    let canonical_directory = canonicalize(receipt_directory)?;
    let canonical_snapshot = canonicalize(&joined)?;
    if canonical_snapshot == canonical_directory
        || !canonical_snapshot.starts_with(&canonical_directory)
        || !canonical_snapshot.is_dir()
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    Ok(canonical_snapshot)
}

fn write_receipt(path: &Path, receipt: &PatchReceipt) -> Result<(), HostIntegrationError> {
    let bytes =
        serde_json::to_vec_pretty(receipt).map_err(|source| HostIntegrationError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    atomic_replace(path, &bytes)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    let parent = path.parent().ok_or_else(|| {
        HostIntegrationError::InvalidBundle(format!("{} has no parent", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    let temporary = parent.join(format!(
        ".agy-byok-{}-{}-next",
        std::process::id(),
        unix_time_ms()
    ));
    fs::write(&temporary, bytes).map_err(|source| io_error(&temporary, source))?;
    fs::rename(&temporary, path).map_err(|source| io_error(path, source))
}

fn write_file_in_place(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|source| io_error(path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error(path, source))?;
    file.sync_all().map_err(|source| io_error(path, source))
}

fn canonicalize(path: &Path) -> Result<PathBuf, HostIntegrationError> {
    fs::canonicalize(path).map_err(|source| io_error(path, source))
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
