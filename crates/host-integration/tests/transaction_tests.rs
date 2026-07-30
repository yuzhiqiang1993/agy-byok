use host_integration::{
    discover, dry_run, restore, sha256, BundleSnapshotStrategy, CodeSignatureVerifier,
    HostIntegrationError, HostLayout, InstallationState, PatchProfile, PatchReceipt,
    PatchTransactionState,
};
use plist::{Dictionary, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const ORIGINAL: &str = "prefix const endpoint=vendor(); suffix";
const PATCHED: &str = "prefix const endpoint=\"http://127.0.0.1:50999\"; suffix";
const EXECUTABLE: &[u8] = b"synthetic signed executable";

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

struct Fixture {
    _temp: TempDir,
    app_path: PathBuf,
    snapshot_root: PathBuf,
    profile: PatchProfile,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let app_path = temp.path().join("Antigravity IDE.app");
        let contents = app_path.join("Contents");
        let extension_root = contents.join("Resources/app/extensions/antigravity");
        fs::create_dir_all(extension_root.join("dist")).unwrap();
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::create_dir_all(contents.join("Frameworks/Nested.framework")).unwrap();
        fs::write(contents.join("MacOS/Electron"), EXECUTABLE).unwrap();
        fs::write(
            contents.join("Frameworks/Nested.framework/resource.bin"),
            b"nested resource",
        )
        .unwrap();

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
            original_sha256: sha256(ORIGINAL.as_bytes()),
            patched_sha256: sha256(PATCHED.as_bytes()),
            endpoint: "http://127.0.0.1:50999".to_string(),
            anchor: "const endpoint=vendor();".to_string(),
            replacement: "const endpoint=\"http://127.0.0.1:50999\";".to_string(),
            layout: HostLayout::antigravity_ide(),
        };
        let snapshot_root = temp.path().join("snapshots");

        Self {
            _temp: temp,
            app_path,
            snapshot_root,
            profile,
        }
    }

    fn extension_path(&self) -> PathBuf {
        self.app_path.join(&self.profile.layout.extension_entry)
    }

    fn executable_path(&self) -> PathBuf {
        self.app_path.join("Contents/MacOS/Electron")
    }
}

#[test]
fn discovery_and_dry_run_require_exact_profile_and_anchor() {
    let fixture = Fixture::new();
    let installation = discover(&fixture.app_path, &fixture.profile.layout).unwrap();
    assert_eq!(
        fixture.profile.classify(&installation).unwrap(),
        InstallationState::VendorOriginal
    );
    assert_eq!(
        installation.executable_relative_path,
        PathBuf::from("Contents/MacOS/Electron")
    );
    assert_eq!(installation.executable_sha256, sha256(EXECUTABLE));
    assert_eq!(
        dry_run(&fixture.app_path, &fixture.profile).unwrap(),
        PATCHED
    );

    fs::write(fixture.extension_path(), "vendor without anchor").unwrap();
    let error = dry_run(&fixture.app_path, &fixture.profile).unwrap_err();
    assert!(matches!(error, HostIntegrationError::ProfileMismatch(_)));
}

#[test]
fn restore_from_applied_v2_receipt_uses_complete_bundle_snapshot() {
    let fixture = Fixture::new();
    let executable_hash = sha256(&fs::read(fixture.executable_path()).unwrap());
    let receipt_path = prepare_applied_receipt(&fixture);
    let applied_receipt = read_receipt(&receipt_path);

    assert_eq!(applied_receipt.schema_version, 2);
    assert_eq!(applied_receipt.state, PatchTransactionState::Applied);
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        PATCHED
    );
    assert_eq!(
        sha256(&fs::read(fixture.executable_path()).unwrap()),
        executable_hash
    );
    assert_eq!(
        applied_receipt.snapshot_bundle_relative_path,
        PathBuf::from("original.app")
    );
    let snapshot_bundle = receipt_path.parent().unwrap().join("original.app");
    assert_eq!(
        fs::read(snapshot_bundle.join("Contents/MacOS/Electron")).unwrap(),
        EXECUTABLE
    );
    assert_eq!(
        fs::read(snapshot_bundle.join("Contents/Frameworks/Nested.framework/resource.bin"))
            .unwrap(),
        b"nested resource"
    );

    let receipt = restore(
        &fixture.app_path,
        &fixture.profile,
        &receipt_path,
        &FakeVendorVerifier,
    )
    .unwrap();
    assert_eq!(receipt.state, PatchTransactionState::Restored);
    assert!(receipt.restored_at_unix_ms.is_some());
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        ORIGINAL
    );
    assert_eq!(
        sha256(&fs::read(fixture.executable_path()).unwrap()),
        executable_hash
    );
}

#[test]
fn restore_rejects_third_party_modification_and_keeps_receipt_applied() {
    let fixture = Fixture::new();
    let receipt_path = prepare_applied_receipt(&fixture);
    fs::write(fixture.extension_path(), "third-party modification").unwrap();

    let error = restore(
        &fixture.app_path,
        &fixture.profile,
        &receipt_path,
        &FakeVendorVerifier,
    )
    .unwrap_err();
    assert!(matches!(error, HostIntegrationError::HashMismatch { .. }));
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        "third-party modification"
    );
    assert_eq!(
        read_receipt(&receipt_path).state,
        PatchTransactionState::Applied
    );
}

#[test]
fn restore_rejects_snapshot_path_escape() {
    let fixture = Fixture::new();
    let receipt_path = prepare_applied_receipt(&fixture);
    let mut receipt = read_receipt(&receipt_path);
    receipt.snapshot_bundle_relative_path = PathBuf::from("../outside.app");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();

    let error = restore(
        &fixture.app_path,
        &fixture.profile,
        &receipt_path,
        &FakeVendorVerifier,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        HostIntegrationError::ReceiptMismatch | HostIntegrationError::UnsafeRelativePath(_)
    ));
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        PATCHED
    );
}

#[test]
fn receipt_states_have_stable_v2_serialization() {
    let states = [
        (PatchTransactionState::Prepared, "\"prepared\""),
        (PatchTransactionState::Applied, "\"applied\""),
        (PatchTransactionState::Restored, "\"restored\""),
        (PatchTransactionState::RolledBack, "\"rolled_back\""),
        (
            PatchTransactionState::RecoveryRequired,
            "\"recovery_required\"",
        ),
    ];
    for (state, expected) in states {
        assert_eq!(serde_json::to_string(&state).unwrap(), expected);
    }
}

#[test]
fn exact_real_profile_rejects_the_known_legacy_patch_hash() {
    let profile = PatchProfile::antigravity_ide_2_1_1();
    let installation = host_integration::HostInstallation {
        app_path: PathBuf::from("/Applications/Antigravity IDE.app"),
        bundle_id: "com.google.antigravity-ide".to_string(),
        app_version: "2.1.1".to_string(),
        extension_version: "0.2.0".to_string(),
        extension_sha256: "13d5d05321a341b6e99b0eb59d3d3fe12af79c51cbd953a8dd4d100bc251b7d8"
            .to_string(),
        executable_relative_path: PathBuf::from("Contents/MacOS/Electron"),
        executable_sha256: "synthetic-executable-hash".to_string(),
    };
    assert_eq!(
        profile.classify(&installation).unwrap(),
        InstallationState::Modified
    );
    assert!(profile.validate_for_apply(&installation).is_err());
}

fn prepare_applied_receipt(fixture: &Fixture) -> PathBuf {
    let installation = discover(&fixture.app_path, &fixture.profile.layout).unwrap();
    let receipt_directory = fixture.snapshot_root.join("test-transaction");
    let snapshot_bundle = receipt_directory.join("original.app");
    copy_tree(&fixture.app_path, &snapshot_bundle).unwrap();
    fs::write(fixture.extension_path(), PATCHED).unwrap();

    let receipt = PatchReceipt {
        schema_version: 2,
        state: PatchTransactionState::Applied,
        profile_id: fixture.profile.id.clone(),
        app_path: fs::canonicalize(&fixture.app_path).unwrap(),
        bundle_id: installation.bundle_id,
        app_version: installation.app_version,
        extension_version: installation.extension_version,
        extension_relative_path: fixture.profile.layout.extension_entry.clone(),
        original_sha256: installation.extension_sha256,
        patched_sha256: fixture.profile.patched_sha256.clone(),
        executable_relative_path: installation.executable_relative_path,
        executable_sha256: installation.executable_sha256,
        endpoint: fixture.profile.endpoint.clone(),
        snapshot_bundle_relative_path: PathBuf::from("original.app"),
        snapshot_strategy: BundleSnapshotStrategy::ClonePreferredCopyFallback,
        prepared_at_unix_ms: 1,
        applied_at_unix_ms: Some(2),
        restored_at_unix_ms: None,
    };
    let receipt_path = receipt_directory.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    receipt_path
}

fn read_receipt(path: &Path) -> PatchReceipt {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), HostIntegrationError> {
    let metadata =
        fs::symlink_metadata(source).map_err(|source_error| HostIntegrationError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(|source_error| HostIntegrationError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination).unwrap();
        #[cfg(not(unix))]
        return Err(HostIntegrationError::CommandFailed(
            "test snapshots require Unix symlink support".to_string(),
        ));
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
