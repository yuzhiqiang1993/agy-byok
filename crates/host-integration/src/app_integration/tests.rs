use super::*;
use plist::{Dictionary, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct Fixture {
    _temp: tempfile::TempDir,
    app_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let app_path = temp.path().join("Antigravity.app");
        let bin_dir = app_path.join("Contents/Resources/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let info_path = app_path.join("Contents/Info.plist");
        let mut info = Dictionary::new();
        info.insert(
            "CFBundleShortVersionString".to_string(),
            Value::String("1.2.3".to_string()),
        );
        Value::Dictionary(info).to_file_xml(&info_path).unwrap();
        let dummy_binary = bin_dir.join("language_server");
        fs::write(&dummy_binary, "#!/bin/sh\necho real_binary").unwrap();
        fs::set_permissions(&dummy_binary, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            _temp: temp,
            app_path,
        }
    }
}

#[test]
fn enables_and_disables_app_wrapper_integration() {
    let fixture = Fixture::new();
    let endpoint = "http://127.0.0.1:56066";

    let status = inspect_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(status.state, AppIntegrationState::Disabled);

    let enabled = enable_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(enabled.state, AppIntegrationState::Managed);
    assert!(enabled.endpoint_matches);
    assert_eq!(enabled.app_version.as_deref(), Some("1.2.3"));

    let wrapper_content = fs::read_to_string(
        fixture
            .app_path
            .join("Contents/Resources/bin/language_server"),
    )
    .unwrap();
    assert!(wrapper_content.contains(WRAPPER_MARKER));
    assert!(wrapper_content.contains(endpoint));
    assert!(fixture
        .app_path
        .join("Contents/Resources/bin/language_server.real")
        .exists());
    assert!(fixture
        .app_path
        .join("Contents/Resources/bin/.agy-byok-language-server.json")
        .exists());

    let mismatch = inspect_app_integration(&fixture.app_path, "http://127.0.0.1:56067").unwrap();
    assert_eq!(mismatch.state, AppIntegrationState::Mismatch);
    assert_eq!(mismatch.configured_endpoint.as_deref(), Some(endpoint));

    let disabled = disable_app_integration(&fixture.app_path, "http://127.0.0.1:56067").unwrap();
    assert_eq!(disabled.state, AppIntegrationState::Disabled);
    assert!(!fixture
        .app_path
        .join("Contents/Resources/bin/language_server.real")
        .exists());
    assert!(!fixture
        .app_path
        .join("Contents/Resources/bin/.agy-byok-language-server.json")
        .exists());
    let restored_content = fs::read_to_string(
        fixture
            .app_path
            .join("Contents/Resources/bin/language_server"),
    )
    .unwrap();
    assert!(restored_content.contains("real_binary"));
}

#[test]
fn restores_legacy_wrapper_and_can_reenable_with_receipt() {
    let fixture = Fixture::new();
    let endpoint = "http://127.0.0.1:56066";
    let bin_dir = fixture.app_path.join("Contents/Resources/bin");
    fs::rename(
        bin_dir.join("language_server"),
        bin_dir.join("language_server.real"),
    )
    .unwrap();
    let legacy_wrapper = format!(
        r#"#!/bin/bash
DIR="$(cd "$(dirname "$0")" && pwd)"
ARGS=()
for arg in "$@"; do
    if [ "$arg" = "{TARGET_OFFICIAL_ENDPOINT}" ]; then
        ARGS+=("{endpoint}")
    else
        ARGS+=("$arg")
    fi
done
exec "$DIR/language_server.real" "${{ARGS[@]}}"
"#
    );
    fs::write(bin_dir.join("language_server"), legacy_wrapper).unwrap();
    fs::set_permissions(
        bin_dir.join("language_server"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let legacy = inspect_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(legacy.state, AppIntegrationState::Managed);
    assert_eq!(legacy.configured_endpoint.as_deref(), Some(endpoint));
    assert!(legacy.message.contains("旧版"));

    let disabled = disable_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(disabled.state, AppIntegrationState::Disabled);

    let enabled = enable_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(enabled.state, AppIntegrationState::Managed);
    assert!(bin_dir.join(RECEIPT_FILE_NAME).exists());
}

#[test]
fn refuses_external_wrapper_changes() {
    let fixture = Fixture::new();
    let endpoint = "http://127.0.0.1:56066";
    enable_app_integration(&fixture.app_path, endpoint).unwrap();
    let wrapper_path = fixture
        .app_path
        .join("Contents/Resources/bin/language_server");
    fs::OpenOptions::new()
        .append(true)
        .open(&wrapper_path)
        .unwrap()
        .write_all(b"# external change\n")
        .unwrap();

    let status = inspect_app_integration(&fixture.app_path, endpoint).unwrap();
    assert_eq!(status.state, AppIntegrationState::Conflict);
    assert!(disable_app_integration(&fixture.app_path, endpoint).is_err());
}

#[test]
fn treats_non_utf8_original_binary_as_official_mode() {
    let fixture = Fixture::new();
    let binary_path = fixture
        .app_path
        .join("Contents/Resources/bin/language_server");
    fs::write(&binary_path, [0_u8, 159, 146, 150, 255]).unwrap();

    let status = inspect_app_integration(&fixture.app_path, "http://127.0.0.1:56066").unwrap();
    assert_eq!(status.state, AppIntegrationState::Disabled);
    assert_eq!(
        status.message,
        "原始 language_server 已就位，App 使用官方服务"
    );
}

#[test]
fn refuses_non_local_endpoints() {
    let fixture = Fixture::new();
    let result = enable_app_integration(&fixture.app_path, "https://example.com");
    assert!(result.is_err());
}
