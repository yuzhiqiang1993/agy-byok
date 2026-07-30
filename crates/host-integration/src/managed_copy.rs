use crate::discovery::{discover, HostInstallation};
use crate::error::{io_error, HostIntegrationError};
use crate::profile::{safe_join, PatchProfile};
use crate::signing::{CodeSignatureVerifier, MacOsCodeSignatureVerifier};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MANAGED_APP_NAME: &str = "Antigravity IDE.app";
pub const MANAGED_RECEIPT_FILE: &str = "managed-copy-receipt.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedCopyState {
    Ready,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCopyReceipt {
    pub schema_version: u32,
    pub state: ManagedCopyState,
    pub profile_id: String,
    pub source_app_path: PathBuf,
    pub managed_app_path: PathBuf,
    pub bundle_id: String,
    pub app_version: String,
    pub extension_version: String,
    pub source_extension_sha256: String,
    pub patched_extension_sha256: String,
    pub executable_relative_path: PathBuf,
    pub source_executable_sha256: String,
    pub managed_executable_sha256: String,
    pub quarantine_removed: bool,
    pub created_at_unix_ms: u128,
    pub removed_at_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedCopyResult {
    pub receipt: ManagedCopyReceipt,
    pub receipt_path: PathBuf,
}

trait ManagedCopyPlatform {
    fn clone_bundle(
        &self,
        source_app: &Path,
        destination_app: &Path,
    ) -> Result<(), HostIntegrationError>;

    fn sign_adhoc(&self, app_path: &Path) -> Result<(), HostIntegrationError>;

    fn verify_adhoc(
        &self,
        app_path: &Path,
        expected_bundle_id: &str,
    ) -> Result<(), HostIntegrationError>;

    fn remove_quarantine_if_present(&self, app_path: &Path) -> Result<bool, HostIntegrationError>;
}

#[derive(Debug, Default, Clone, Copy)]
struct MacOsManagedCopyPlatform;

impl ManagedCopyPlatform for MacOsManagedCopyPlatform {
    fn clone_bundle(
        &self,
        source_app: &Path,
        destination_app: &Path,
    ) -> Result<(), HostIntegrationError> {
        ensure_macos()?;
        let cloned = Command::new("/bin/cp")
            .args(["-c", "-a"])
            .arg(source_app)
            .arg(destination_app)
            .output()
            .map_err(|error| command_start_error("APFS clone", error))?;
        if cloned.status.success() {
            return Ok(());
        }
        remove_path_if_present(destination_app);

        let copied = Command::new("/bin/cp")
            .arg("-a")
            .arg(source_app)
            .arg(destination_app)
            .output()
            .map_err(|error| command_start_error("bundle copy fallback", error))?;
        ensure_command_success("bundle copy fallback", copied)
    }

    fn sign_adhoc(&self, app_path: &Path) -> Result<(), HostIntegrationError> {
        ensure_macos()?;
        let output = Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(app_path)
            .output()
            .map_err(|error| command_start_error("managed copy ad-hoc signing", error))?;
        ensure_command_success("managed copy ad-hoc signing", output)
    }

    fn verify_adhoc(
        &self,
        app_path: &Path,
        expected_bundle_id: &str,
    ) -> Result<(), HostIntegrationError> {
        ensure_macos()?;
        let verification = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict", "--all-architectures"])
            .arg(app_path)
            .output()
            .map_err(|error| command_start_error("managed copy signature verification", error))?;
        ensure_command_success("managed copy signature verification", verification)?;

        let details = Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=4"])
            .arg(app_path)
            .output()
            .map_err(|error| command_start_error("managed copy signature inspection", error))?;
        ensure_command_success("managed copy signature inspection", details.clone())?;
        let output = command_output(&details);
        let expected_identifier = format!("Identifier={expected_bundle_id}");
        let has_expected_identifier = output.lines().any(|line| line == expected_identifier);
        let is_ad_hoc = output.lines().any(|line| line == "Signature=adhoc");
        if !has_expected_identifier || !is_ad_hoc {
            return Err(HostIntegrationError::CommandFailed(format!(
                "managed copy does not have the expected ad-hoc identity: {output}"
            )));
        }
        Ok(())
    }

    fn remove_quarantine_if_present(&self, app_path: &Path) -> Result<bool, HostIntegrationError> {
        ensure_macos()?;
        let inspection = Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(app_path)
            .output()
            .map_err(|error| command_start_error("managed copy quarantine inspection", error))?;
        if !inspection.status.success() {
            return Ok(false);
        }

        let removal = Command::new("/usr/bin/xattr")
            .args(["-d", "-r", "com.apple.quarantine"])
            .arg(app_path)
            .output()
            .map_err(|error| command_start_error("managed copy quarantine removal", error))?;
        ensure_command_success("managed copy quarantine removal", removal)?;
        Ok(true)
    }
}

pub fn create_managed_copy(
    source_app: impl AsRef<Path>,
    managed_root: impl AsRef<Path>,
    profile: &PatchProfile,
) -> Result<ManagedCopyResult, HostIntegrationError> {
    create_managed_copy_with_platform(
        source_app.as_ref(),
        managed_root.as_ref(),
        profile,
        &MacOsCodeSignatureVerifier,
        &MacOsManagedCopyPlatform,
    )
}

pub fn inspect_managed_copy(
    managed_root: impl AsRef<Path>,
    profile: &PatchProfile,
) -> Result<Option<ManagedCopyReceipt>, HostIntegrationError> {
    inspect_managed_copy_with_platform(managed_root.as_ref(), profile, &MacOsManagedCopyPlatform)
}

pub fn remove_managed_copy(
    managed_root: impl AsRef<Path>,
    profile: &PatchProfile,
) -> Result<ManagedCopyReceipt, HostIntegrationError> {
    remove_managed_copy_with_platform(managed_root.as_ref(), profile, &MacOsManagedCopyPlatform)
}

fn create_managed_copy_with_platform(
    source_app: &Path,
    managed_root: &Path,
    profile: &PatchProfile,
    vendor_verifier: &dyn CodeSignatureVerifier,
    platform: &dyn ManagedCopyPlatform,
) -> Result<ManagedCopyResult, HostIntegrationError> {
    let source = discover(source_app, &profile.layout)?;
    profile.validate_for_apply(&source)?;
    vendor_verifier.verify_vendor(&source.app_path, &profile.bundle_id)?;

    prepare_managed_root(managed_root)?;
    let managed_app_path = managed_root.join(MANAGED_APP_NAME);
    let receipt_path = managed_root.join(MANAGED_RECEIPT_FILE);
    reject_existing_managed_target(&managed_app_path, &receipt_path)?;

    let staging_path = managed_root.join(format!(
        ".{MANAGED_APP_NAME}.staging-{}-{}",
        std::process::id(),
        unix_time_ms()
    ));
    if staging_path.exists() || staging_path.is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "managed copy staging path already exists: {}",
            staging_path.display()
        )));
    }

    let result = (|| {
        platform.clone_bundle(&source.app_path, &staging_path)?;
        let cloned = discover(&staging_path, &profile.layout)?;
        validate_source_clone(&source, &cloned)?;
        vendor_verifier.verify_vendor(&cloned.app_path, &profile.bundle_id)?;

        let source_extension = safe_join(&source.app_path, &profile.layout.extension_entry)?;
        let source_text = fs::read_to_string(&source_extension)
            .map_err(|error| io_error(&source_extension, error))?;
        let candidate = profile.create_candidate(&source_text)?;
        let staged_extension = safe_join(&staging_path, &profile.layout.extension_entry)?;
        write_file_in_place(&staged_extension, candidate.as_bytes())?;

        let patched = discover(&staging_path, &profile.layout)?;
        if patched.extension_sha256 != profile.patched_sha256 {
            return Err(HostIntegrationError::HashMismatch {
                expected: profile.patched_sha256.clone(),
                actual: patched.extension_sha256,
            });
        }

        platform.sign_adhoc(&staging_path)?;
        platform.verify_adhoc(&staging_path, &profile.bundle_id)?;
        let quarantine_removed = platform.remove_quarantine_if_present(&staging_path)?;
        platform.verify_adhoc(&staging_path, &profile.bundle_id)?;

        fs::rename(&staging_path, &managed_app_path)
            .map_err(|error| io_error(&managed_app_path, error))?;
        let managed = discover(&managed_app_path, &profile.layout)?;
        if managed.extension_sha256 != profile.patched_sha256 {
            return Err(HostIntegrationError::HashMismatch {
                expected: profile.patched_sha256.clone(),
                actual: managed.extension_sha256,
            });
        }
        platform.verify_adhoc(&managed_app_path, &profile.bundle_id)?;

        let source_after = discover(&source.app_path, &profile.layout)?;
        validate_source_clone(&source, &source_after)?;
        vendor_verifier.verify_vendor(&source_after.app_path, &profile.bundle_id)?;

        let receipt = ManagedCopyReceipt {
            schema_version: 1,
            state: ManagedCopyState::Ready,
            profile_id: profile.id.clone(),
            source_app_path: canonicalize(&source.app_path)?,
            managed_app_path: canonicalize(&managed_app_path)?,
            bundle_id: source.bundle_id,
            app_version: source.app_version,
            extension_version: source.extension_version,
            source_extension_sha256: source.extension_sha256,
            patched_extension_sha256: managed.extension_sha256,
            executable_relative_path: source.executable_relative_path,
            source_executable_sha256: source.executable_sha256,
            managed_executable_sha256: managed.executable_sha256,
            quarantine_removed,
            created_at_unix_ms: unix_time_ms(),
            removed_at_unix_ms: None,
        };
        write_receipt(&receipt_path, &receipt)?;
        Ok(ManagedCopyResult {
            receipt,
            receipt_path: receipt_path.clone(),
        })
    })();

    if result.is_err() {
        remove_path_if_present(&staging_path);
        remove_path_if_present(&managed_app_path);
    }
    result
}

fn inspect_managed_copy_with_platform(
    managed_root: &Path,
    profile: &PatchProfile,
    platform: &dyn ManagedCopyPlatform,
) -> Result<Option<ManagedCopyReceipt>, HostIntegrationError> {
    let managed_app_path = managed_root.join(MANAGED_APP_NAME);
    let receipt_path = managed_root.join(MANAGED_RECEIPT_FILE);
    if !receipt_path.is_file() {
        if managed_app_path.exists() || managed_app_path.is_symlink() {
            return Err(HostIntegrationError::InvalidBundle(format!(
                "unmanaged application exists at {}",
                managed_app_path.display()
            )));
        }
        return Ok(None);
    }

    let receipt = read_receipt(&receipt_path)?;
    validate_managed_receipt(managed_root, profile, &receipt)?;
    if receipt.state == ManagedCopyState::Removed {
        if managed_app_path.exists() || managed_app_path.is_symlink() {
            return Err(HostIntegrationError::ReceiptMismatch);
        }
        return Ok(None);
    }

    let managed = discover(&receipt.managed_app_path, &profile.layout)?;
    validate_managed_installation(&managed, &receipt)?;
    platform.verify_adhoc(&managed.app_path, &profile.bundle_id)?;
    Ok(Some(receipt))
}

fn remove_managed_copy_with_platform(
    managed_root: &Path,
    profile: &PatchProfile,
    platform: &dyn ManagedCopyPlatform,
) -> Result<ManagedCopyReceipt, HostIntegrationError> {
    let receipt_path = managed_root.join(MANAGED_RECEIPT_FILE);
    let mut receipt = read_receipt(&receipt_path)?;
    validate_managed_receipt(managed_root, profile, &receipt)?;
    if receipt.state != ManagedCopyState::Ready {
        return Err(HostIntegrationError::ReceiptMismatch);
    }

    let managed = discover(&receipt.managed_app_path, &profile.layout)?;
    validate_managed_installation(&managed, &receipt)?;
    platform.verify_adhoc(&managed.app_path, &profile.bundle_id)?;
    fs::remove_dir_all(&receipt.managed_app_path)
        .map_err(|error| io_error(&receipt.managed_app_path, error))?;

    receipt.state = ManagedCopyState::Removed;
    receipt.removed_at_unix_ms = Some(unix_time_ms());
    write_receipt(&receipt_path, &receipt)?;
    Ok(receipt)
}

fn prepare_managed_root(managed_root: &Path) -> Result<(), HostIntegrationError> {
    if managed_root.exists() || managed_root.is_symlink() {
        let metadata =
            fs::symlink_metadata(managed_root).map_err(|error| io_error(managed_root, error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(HostIntegrationError::InvalidBundle(format!(
                "managed root is not a regular directory: {}",
                managed_root.display()
            )));
        }
    } else {
        fs::create_dir_all(managed_root).map_err(|error| io_error(managed_root, error))?;
    }
    Ok(())
}

fn reject_existing_managed_target(
    managed_app_path: &Path,
    receipt_path: &Path,
) -> Result<(), HostIntegrationError> {
    if managed_app_path.exists() || managed_app_path.is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "managed application already exists: {}",
            managed_app_path.display()
        )));
    }
    if receipt_path.is_file() {
        let receipt = read_receipt(receipt_path)?;
        if receipt.state != ManagedCopyState::Removed {
            return Err(HostIntegrationError::ReceiptMismatch);
        }
    } else if receipt_path.exists() || receipt_path.is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "managed receipt path is not a regular file: {}",
            receipt_path.display()
        )));
    }
    Ok(())
}

fn validate_source_clone(
    source: &HostInstallation,
    candidate: &HostInstallation,
) -> Result<(), HostIntegrationError> {
    if source.bundle_id != candidate.bundle_id
        || source.app_version != candidate.app_version
        || source.extension_version != candidate.extension_version
        || source.extension_sha256 != candidate.extension_sha256
        || source.executable_relative_path != candidate.executable_relative_path
        || source.executable_sha256 != candidate.executable_sha256
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    Ok(())
}

fn validate_managed_receipt(
    managed_root: &Path,
    profile: &PatchProfile,
    receipt: &ManagedCopyReceipt,
) -> Result<(), HostIntegrationError> {
    let expected_managed_path = canonicalize(managed_root)?.join(MANAGED_APP_NAME);
    if receipt.schema_version != 1
        || receipt.profile_id != profile.id
        || receipt.bundle_id != profile.bundle_id
        || receipt.app_version != profile.app_version
        || receipt.extension_version != profile.extension_version
        || receipt.source_extension_sha256 != profile.original_sha256
        || receipt.patched_extension_sha256 != profile.patched_sha256
        || receipt.managed_app_path != expected_managed_path
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    Ok(())
}

fn validate_managed_installation(
    managed: &HostInstallation,
    receipt: &ManagedCopyReceipt,
) -> Result<(), HostIntegrationError> {
    if managed.app_path != receipt.managed_app_path
        || managed.bundle_id != receipt.bundle_id
        || managed.app_version != receipt.app_version
        || managed.extension_version != receipt.extension_version
        || managed.extension_sha256 != receipt.patched_extension_sha256
        || managed.executable_relative_path != receipt.executable_relative_path
        || managed.executable_sha256 != receipt.managed_executable_sha256
    {
        return Err(HostIntegrationError::ReceiptMismatch);
    }
    Ok(())
}

fn read_receipt(path: &Path) -> Result<ManagedCopyReceipt, HostIntegrationError> {
    let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
    serde_json::from_slice(&bytes).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn write_receipt(path: &Path, receipt: &ManagedCopyReceipt) -> Result<(), HostIntegrationError> {
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
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temporary = parent.join(format!(
        ".managed-copy-receipt-{}-{}.next",
        std::process::id(),
        unix_time_ms()
    ));
    fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io_error(path, error))
}

fn write_file_in_place(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
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
        .map_err(|error| io_error(path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error(path, error))?;
    file.sync_all().map_err(|error| io_error(path, error))
}

fn remove_path_if_present(path: &Path) {
    if path.is_dir() && !path.is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() || path.is_symlink() {
        let _ = fs::remove_file(path);
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf, HostIntegrationError> {
    fs::canonicalize(path).map_err(|error| io_error(path, error))
}

fn ensure_macos() -> Result<(), HostIntegrationError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(HostIntegrationError::CommandFailed(
            "managed Antigravity IDE copies are only supported on macOS".to_string(),
        ))
    }
}

fn command_start_error(operation: &str, error: std::io::Error) -> HostIntegrationError {
    HostIntegrationError::CommandFailed(format!("failed to start {operation}: {error}"))
}

fn ensure_command_success(operation: &str, output: Output) -> Result<(), HostIntegrationError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(HostIntegrationError::CommandFailed(format!(
            "{operation} failed with status {}: {}",
            output.status,
            command_output(&output)
        )))
    }
}

fn command_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}").trim().to_string()
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Value};
    use std::cell::Cell;
    use tempfile::TempDir;

    const ORIGINAL: &str = "prefix const endpoint=vendor(); suffix";
    const PATCHED: &str = "prefix const endpoint=\"http://127.0.0.1:50999\"; suffix";
    const EXECUTABLE: &[u8] = b"vendor executable";
    const SIGNED_EXECUTABLE: &[u8] = b"ad-hoc signed executable";

    #[derive(Default)]
    struct FakeVendorVerifier;

    impl CodeSignatureVerifier for FakeVendorVerifier {
        fn verify_vendor(
            &self,
            _app_path: &Path,
            _expected_bundle_id: &str,
        ) -> Result<(), HostIntegrationError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakePlatform {
        fail_signing: bool,
        quarantine_removed: Cell<bool>,
    }

    impl ManagedCopyPlatform for FakePlatform {
        fn clone_bundle(
            &self,
            source_app: &Path,
            destination_app: &Path,
        ) -> Result<(), HostIntegrationError> {
            copy_tree(source_app, destination_app)
        }

        fn sign_adhoc(&self, app_path: &Path) -> Result<(), HostIntegrationError> {
            if self.fail_signing {
                return Err(HostIntegrationError::CommandFailed(
                    "synthetic signing failure".to_string(),
                ));
            }
            fs::write(app_path.join("Contents/MacOS/Electron"), SIGNED_EXECUTABLE).unwrap();
            Ok(())
        }

        fn verify_adhoc(
            &self,
            _app_path: &Path,
            _expected_bundle_id: &str,
        ) -> Result<(), HostIntegrationError> {
            Ok(())
        }

        fn remove_quarantine_if_present(
            &self,
            _app_path: &Path,
        ) -> Result<bool, HostIntegrationError> {
            self.quarantine_removed.set(true);
            Ok(true)
        }
    }

    struct Fixture {
        _temp: TempDir,
        source_app: PathBuf,
        managed_root: PathBuf,
        profile: PatchProfile,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let source_app = temp.path().join("Vendor.app");
            let contents = source_app.join("Contents");
            let extension_root = contents.join("Resources/app/extensions/antigravity");
            fs::create_dir_all(extension_root.join("dist")).unwrap();
            fs::create_dir_all(contents.join("MacOS")).unwrap();
            fs::write(contents.join("MacOS/Electron"), EXECUTABLE).unwrap();

            let mut info = Dictionary::new();
            info.insert(
                "CFBundleIdentifier".to_string(),
                Value::String("com.example.ide".to_string()),
            );
            info.insert(
                "CFBundleShortVersionString".to_string(),
                Value::String("1.2.3".to_string()),
            );
            info.insert(
                "CFBundleExecutable".to_string(),
                Value::String("Electron".to_string()),
            );
            Value::Dictionary(info)
                .to_file_xml(contents.join("Info.plist"))
                .unwrap();
            fs::write(
                extension_root.join("package.json"),
                br#"{"version":"0.4.5"}"#,
            )
            .unwrap();
            fs::write(extension_root.join("dist/extension.js"), ORIGINAL).unwrap();

            let profile = PatchProfile {
                id: "test-profile".to_string(),
                bundle_id: "com.example.ide".to_string(),
                app_version: "1.2.3".to_string(),
                extension_version: "0.4.5".to_string(),
                original_sha256: crate::sha256(ORIGINAL.as_bytes()),
                patched_sha256: crate::sha256(PATCHED.as_bytes()),
                endpoint: "http://127.0.0.1:50999".to_string(),
                anchor: "const endpoint=vendor();".to_string(),
                replacement: "const endpoint=\"http://127.0.0.1:50999\";".to_string(),
                layout: crate::HostLayout::antigravity_ide(),
            };
            let managed_root = temp.path().join("Applications/AGY BYOK");
            Self {
                _temp: temp,
                source_app,
                managed_root,
                profile,
            }
        }

        fn source_extension(&self) -> PathBuf {
            self.source_app.join(&self.profile.layout.extension_entry)
        }

        fn managed_app(&self) -> PathBuf {
            self.managed_root.join(MANAGED_APP_NAME)
        }
    }

    #[test]
    fn managed_copy_patches_and_signs_only_the_clone() {
        let fixture = Fixture::new();
        let platform = FakePlatform::default();
        let result = create_managed_copy_with_platform(
            &fixture.source_app,
            &fixture.managed_root,
            &fixture.profile,
            &FakeVendorVerifier,
            &platform,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(fixture.source_extension()).unwrap(),
            ORIGINAL
        );
        assert_eq!(
            fs::read_to_string(
                fixture
                    .managed_app()
                    .join(&fixture.profile.layout.extension_entry)
            )
            .unwrap(),
            PATCHED
        );
        assert_eq!(result.receipt.state, ManagedCopyState::Ready);
        assert_eq!(
            result.receipt.source_executable_sha256,
            crate::sha256(EXECUTABLE)
        );
        assert_eq!(
            result.receipt.managed_executable_sha256,
            crate::sha256(SIGNED_EXECUTABLE)
        );
        assert!(result.receipt.quarantine_removed);
        assert!(platform.quarantine_removed.get());
        assert_eq!(
            inspect_managed_copy_with_platform(&fixture.managed_root, &fixture.profile, &platform)
                .unwrap()
                .unwrap(),
            result.receipt
        );
    }

    #[test]
    fn signing_failure_cleans_staging_and_final_copy() {
        let fixture = Fixture::new();
        let platform = FakePlatform {
            fail_signing: true,
            ..FakePlatform::default()
        };
        let error = create_managed_copy_with_platform(
            &fixture.source_app,
            &fixture.managed_root,
            &fixture.profile,
            &FakeVendorVerifier,
            &platform,
        )
        .unwrap_err();

        assert!(matches!(error, HostIntegrationError::CommandFailed(_)));
        assert!(!fixture.managed_app().exists());
        assert!(fs::read_dir(&fixture.managed_root)
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("staging")));
        assert_eq!(
            fs::read_to_string(fixture.source_extension()).unwrap(),
            ORIGINAL
        );
    }

    #[test]
    fn existing_unmanaged_copy_is_rejected() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.managed_app()).unwrap();
        let error = create_managed_copy_with_platform(
            &fixture.source_app,
            &fixture.managed_root,
            &fixture.profile,
            &FakeVendorVerifier,
            &FakePlatform::default(),
        )
        .unwrap_err();
        assert!(matches!(error, HostIntegrationError::InvalidBundle(_)));
    }

    #[test]
    fn remove_requires_an_unmodified_managed_copy() {
        let fixture = Fixture::new();
        let platform = FakePlatform::default();
        create_managed_copy_with_platform(
            &fixture.source_app,
            &fixture.managed_root,
            &fixture.profile,
            &FakeVendorVerifier,
            &platform,
        )
        .unwrap();
        let managed_extension = fixture
            .managed_app()
            .join(&fixture.profile.layout.extension_entry);
        fs::write(&managed_extension, "third-party change").unwrap();

        let error =
            remove_managed_copy_with_platform(&fixture.managed_root, &fixture.profile, &platform)
                .unwrap_err();
        assert!(matches!(error, HostIntegrationError::ReceiptMismatch));
        assert!(fixture.managed_app().exists());
    }

    #[test]
    fn remove_deletes_only_the_receipted_copy() {
        let fixture = Fixture::new();
        let platform = FakePlatform::default();
        create_managed_copy_with_platform(
            &fixture.source_app,
            &fixture.managed_root,
            &fixture.profile,
            &FakeVendorVerifier,
            &platform,
        )
        .unwrap();

        let receipt =
            remove_managed_copy_with_platform(&fixture.managed_root, &fixture.profile, &platform)
                .unwrap();
        assert_eq!(receipt.state, ManagedCopyState::Removed);
        assert!(receipt.removed_at_unix_ms.is_some());
        assert!(!fixture.managed_app().exists());
        assert!(fixture.source_app.exists());
        assert!(inspect_managed_copy_with_platform(
            &fixture.managed_root,
            &fixture.profile,
            &platform
        )
        .unwrap()
        .is_none());
    }

    fn copy_tree(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
        let metadata = fs::symlink_metadata(source).map_err(|error| io_error(source, error))?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(source).map_err(|error| io_error(source, error))?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            std::os::unix::fs::symlink(target, destination).unwrap();
            return Ok(());
        }
        if metadata.is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(source, destination).unwrap();
            return Ok(());
        }
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    }
}
