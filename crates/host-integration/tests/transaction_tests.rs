use host_integration::{
    apply, discover, dry_run, restore, sha256, CodeSigner, HostIntegrationError, HostLayout,
    InstallationState, PatchProfile,
};
use plist::{Dictionary, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const ORIGINAL: &str = "prefix const endpoint=vendor(); suffix";
const PATCHED: &str = "prefix const endpoint=\"http://127.0.0.1:50999\"; suffix";

#[derive(Default)]
struct PassSigner;

impl CodeSigner for PassSigner {
    fn sign(&self, _app_path: &Path) -> Result<(), HostIntegrationError> {
        Ok(())
    }

    fn verify(&self, _app_path: &Path) -> Result<(), HostIntegrationError> {
        Ok(())
    }
}

struct FailSign;

impl CodeSigner for FailSign {
    fn sign(&self, _app_path: &Path) -> Result<(), HostIntegrationError> {
        Err(HostIntegrationError::CommandFailed(
            "synthetic signing failure".to_string(),
        ))
    }

    fn verify(&self, _app_path: &Path) -> Result<(), HostIntegrationError> {
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
        fs::create_dir_all(contents.join("_CodeSignature")).unwrap();
        fs::write(contents.join("_CodeSignature/CodeResources"), b"signature").unwrap();
        fs::write(contents.join("CodeResources"), b"outer-signature").unwrap();

        let mut info = Dictionary::new();
        info.insert(
            "CFBundleIdentifier".to_string(),
            Value::String("com.example.ide".to_string()),
        );
        info.insert(
            "CFBundleShortVersionString".to_string(),
            Value::String("1.2.3".to_string()),
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

        let layout = HostLayout::antigravity_ide();
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
            layout,
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
        dry_run(&fixture.app_path, &fixture.profile).unwrap(),
        PATCHED
    );

    fs::write(fixture.extension_path(), "vendor without anchor").unwrap();
    let error = dry_run(&fixture.app_path, &fixture.profile).unwrap_err();
    assert!(matches!(error, HostIntegrationError::ProfileMismatch(_)));
}

#[test]
fn apply_and_restore_round_trip_preserves_original_files() {
    let fixture = Fixture::new();
    let result = apply(
        &fixture.app_path,
        &fixture.profile,
        &fixture.snapshot_root,
        &PassSigner,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        PATCHED
    );
    assert!(result.receipt_path.is_file());

    let receipt = restore(
        &fixture.app_path,
        &fixture.profile,
        &result.receipt_path,
        &PassSigner,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        ORIGINAL
    );
    assert!(receipt.restored_at_unix_ms.is_some());
    assert_eq!(
        fs::read(
            fixture
                .app_path
                .join("Contents/_CodeSignature/CodeResources")
        )
        .unwrap(),
        b"signature"
    );
    assert_eq!(
        fs::read(fixture.app_path.join("Contents/CodeResources")).unwrap(),
        b"outer-signature"
    );
}

#[test]
fn restore_rejects_third_party_modification() {
    let fixture = Fixture::new();
    let result = apply(
        &fixture.app_path,
        &fixture.profile,
        &fixture.snapshot_root,
        &PassSigner,
    )
    .unwrap();
    fs::write(fixture.extension_path(), "third-party modification").unwrap();

    let error = restore(
        &fixture.app_path,
        &fixture.profile,
        &result.receipt_path,
        &PassSigner,
    )
    .unwrap_err();
    assert!(matches!(error, HostIntegrationError::HashMismatch { .. }));
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        "third-party modification"
    );
}

#[test]
fn failed_signing_rolls_back_before_returning_error() {
    let fixture = Fixture::new();
    let error = apply(
        &fixture.app_path,
        &fixture.profile,
        &fixture.snapshot_root,
        &FailSign,
    )
    .unwrap_err();
    assert!(matches!(error, HostIntegrationError::CommandFailed(_)));
    assert_eq!(
        fs::read_to_string(fixture.extension_path()).unwrap(),
        ORIGINAL
    );
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
    };
    assert_eq!(
        profile.classify(&installation).unwrap(),
        InstallationState::Modified
    );
    assert!(profile.validate_for_apply(&installation).is_err());
}
