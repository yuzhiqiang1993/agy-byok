use crate::discovery::{discover, HostInstallation};
use crate::error::{io_error, HostIntegrationError};
use crate::profile::{safe_join, PatchProfile};
use crate::signing::CodeSigner;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RECEIPT_FILE: &str = "receipt.json";
const SNAPSHOT_EXTENSION_FILE: &str = "extension.js";
const SNAPSHOT_SIGNATURE_DIR: &str = "_CodeSignature";
const SNAPSHOT_CODE_RESOURCES: &str = "CodeResources";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchReceipt {
    pub schema_version: u32,
    pub profile_id: String,
    pub app_path: PathBuf,
    pub bundle_id: String,
    pub app_version: String,
    pub extension_version: String,
    pub extension_relative_path: PathBuf,
    pub original_sha256: String,
    pub patched_sha256: String,
    pub endpoint: String,
    pub snapshot_directory: PathBuf,
    pub signature_directory_existed: bool,
    pub code_resources_existed: bool,
    pub applied_at_unix_ms: u128,
    pub restored_at_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    pub receipt: PatchReceipt,
    pub receipt_path: PathBuf,
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

pub fn apply(
    app_path: impl AsRef<Path>,
    profile: &PatchProfile,
    snapshot_root: impl AsRef<Path>,
    signer: &dyn CodeSigner,
) -> Result<ApplyResult, HostIntegrationError> {
    let installation = discover(app_path, &profile.layout)?;
    profile.validate_for_apply(&installation)?;
    let extension_path = safe_join(&installation.app_path, &profile.layout.extension_entry)?;
    let source =
        fs::read_to_string(&extension_path).map_err(|source| io_error(&extension_path, source))?;
    let candidate = profile.create_candidate(&source)?;

    let snapshot_directory = create_snapshot_directory(snapshot_root.as_ref(), profile)?;
    let signature_path = safe_join(&installation.app_path, &profile.layout.signature_directory)?;
    let code_resources_path = safe_join(&installation.app_path, &profile.layout.code_resources)?;
    let signature_directory_existed = signature_path.exists();
    let code_resources_existed = code_resources_path.exists() || code_resources_path.is_symlink();

    copy_file(
        &extension_path,
        &snapshot_directory.join(SNAPSHOT_EXTENSION_FILE),
    )?;
    if signature_directory_existed {
        copy_tree(
            &signature_path,
            &snapshot_directory.join(SNAPSHOT_SIGNATURE_DIR),
        )?;
    }
    if code_resources_existed {
        copy_tree(
            &code_resources_path,
            &snapshot_directory.join(SNAPSHOT_CODE_RESOURCES),
        )?;
    }

    let receipt_path = snapshot_directory.join(RECEIPT_FILE);
    let receipt = PatchReceipt {
        schema_version: 1,
        profile_id: profile.id.clone(),
        app_path: installation.app_path.clone(),
        bundle_id: installation.bundle_id.clone(),
        app_version: installation.app_version.clone(),
        extension_version: installation.extension_version.clone(),
        extension_relative_path: profile.layout.extension_entry.clone(),
        original_sha256: installation.extension_sha256.clone(),
        patched_sha256: profile.patched_sha256.clone(),
        endpoint: profile.endpoint.clone(),
        snapshot_directory: snapshot_directory.clone(),
        signature_directory_existed,
        code_resources_existed,
        applied_at_unix_ms: unix_time_ms(),
        restored_at_unix_ms: None,
    };
    write_receipt(&receipt_path, &receipt)?;

    if let Err(apply_error) = atomic_replace(&extension_path, candidate.as_bytes())
        .and_then(|_| signer.sign(&installation.app_path))
        .and_then(|_| signer.verify(&installation.app_path))
    {
        if let Err(rollback_error) = restore_snapshot(&receipt, profile, signer) {
            return Err(HostIntegrationError::CommandFailed(format!(
                "apply failed: {apply_error}; rollback also failed: {rollback_error}"
            )));
        }
        return Err(apply_error);
    }

    let patched = discover(&installation.app_path, &profile.layout)?;
    if patched.extension_sha256 != profile.patched_sha256 {
        let verification_error = HostIntegrationError::HashMismatch {
            expected: profile.patched_sha256.clone(),
            actual: patched.extension_sha256,
        };
        if let Err(rollback_error) = restore_snapshot(&receipt, profile, signer) {
            return Err(HostIntegrationError::CommandFailed(format!(
                "apply verification failed: {verification_error}; rollback also failed: {rollback_error}"
            )));
        }
        return Err(verification_error);
    }

    Ok(ApplyResult {
        receipt,
        receipt_path,
    })
}

pub fn restore(
    app_path: impl AsRef<Path>,
    profile: &PatchProfile,
    receipt_path: impl AsRef<Path>,
    signer: &dyn CodeSigner,
) -> Result<PatchReceipt, HostIntegrationError> {
    let app_path = app_path.as_ref();
    let receipt_path = receipt_path.as_ref();
    let receipt_bytes = fs::read(receipt_path).map_err(|source| io_error(receipt_path, source))?;
    let mut receipt: PatchReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|source| HostIntegrationError::Json {
            path: receipt_path.to_path_buf(),
            source,
        })?;
    validate_receipt(app_path, profile, &receipt)?;

    let current = discover(app_path, &profile.layout)?;
    if current.bundle_id != receipt.bundle_id
        || current.app_version != receipt.app_version
        || current.extension_version != receipt.extension_version
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    if current.extension_sha256 != receipt.patched_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.patched_sha256.clone(),
            actual: current.extension_sha256,
        });
    }

    restore_snapshot(&receipt, profile, signer)?;
    let restored = discover(app_path, &profile.layout)?;
    if restored.extension_sha256 != receipt.original_sha256 {
        return Err(HostIntegrationError::HashMismatch {
            expected: receipt.original_sha256.clone(),
            actual: restored.extension_sha256,
        });
    }

    receipt.restored_at_unix_ms = Some(unix_time_ms());
    write_receipt(receipt_path, &receipt)?;
    Ok(receipt)
}

fn validate_receipt(
    app_path: &Path,
    profile: &PatchProfile,
    receipt: &PatchReceipt,
) -> Result<(), HostIntegrationError> {
    let requested_app = canonicalize(app_path)?;
    let receipt_app = canonicalize(&receipt.app_path)?;
    if receipt.schema_version != 1
        || receipt.profile_id != profile.id
        || requested_app != receipt_app
        || receipt.bundle_id != profile.bundle_id
        || receipt.app_version != profile.app_version
        || receipt.extension_version != profile.extension_version
        || receipt.extension_relative_path != profile.layout.extension_entry
        || receipt.original_sha256 != profile.original_sha256
        || receipt.patched_sha256 != profile.patched_sha256
        || receipt.endpoint != profile.endpoint
        || receipt.restored_at_unix_ms.is_some()
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    Ok(())
}

fn restore_snapshot(
    receipt: &PatchReceipt,
    profile: &PatchProfile,
    signer: &dyn CodeSigner,
) -> Result<(), HostIntegrationError> {
    let extension_path = safe_join(&receipt.app_path, &profile.layout.extension_entry)?;
    atomic_replace_from(
        &extension_path,
        &receipt.snapshot_directory.join(SNAPSHOT_EXTENSION_FILE),
    )?;

    let signature_path = safe_join(&receipt.app_path, &profile.layout.signature_directory)?;
    remove_path_if_exists(&signature_path)?;
    if receipt.signature_directory_existed {
        copy_tree(
            &receipt.snapshot_directory.join(SNAPSHOT_SIGNATURE_DIR),
            &signature_path,
        )?;
    }

    let code_resources_path = safe_join(&receipt.app_path, &profile.layout.code_resources)?;
    remove_path_if_exists(&code_resources_path)?;
    if receipt.code_resources_existed {
        copy_tree(
            &receipt.snapshot_directory.join(SNAPSHOT_CODE_RESOURCES),
            &code_resources_path,
        )?;
    }

    signer.verify(&receipt.app_path)
}

fn create_snapshot_directory(
    snapshot_root: &Path,
    profile: &PatchProfile,
) -> Result<PathBuf, HostIntegrationError> {
    fs::create_dir_all(snapshot_root).map_err(|source| io_error(snapshot_root, source))?;
    for attempt in 0..100_u32 {
        let name = format!(
            "{}-{}-{}-{attempt}",
            profile.id,
            unix_time_ms(),
            std::process::id()
        );
        let path = snapshot_root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(path, error)),
        }
    }
    Err(HostIntegrationError::CommandFailed(
        "could not allocate a unique snapshot directory".to_string(),
    ))
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
    if path.exists() {
        fs::copy(path, &temporary).map_err(|source| io_error(&temporary, source))?;
    }
    fs::write(&temporary, bytes).map_err(|source| io_error(&temporary, source))?;
    fs::rename(&temporary, path).map_err(|source| io_error(path, source))
}

fn atomic_replace_from(path: &Path, source_path: &Path) -> Result<(), HostIntegrationError> {
    let bytes = fs::read(source_path).map_err(|source| io_error(source_path, source))?;
    atomic_replace(path, &bytes)
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| io_error(destination, error))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| io_error(source, error))?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(|error| io_error(source, error))?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)
            .map_err(|error| io_error(destination, error))?;
        #[cfg(not(unix))]
        return Err(HostIntegrationError::CommandFailed(
            "symbolic-link snapshots require a Unix host".to_string(),
        ));
        return Ok(());
    }
    if metadata.is_file() {
        return copy_file(source, destination);
    }
    fs::create_dir_all(destination).map_err(|error| io_error(destination, error))?;
    for entry in fs::read_dir(source).map_err(|error| io_error(source, error))? {
        let entry = entry.map_err(|error| io_error(source, error))?;
        copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), HostIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|error| io_error(path, error))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| io_error(path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
    }
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

#[allow(dead_code)]
fn _assert_installation_is_send_sync(_: &HostInstallation) {}
