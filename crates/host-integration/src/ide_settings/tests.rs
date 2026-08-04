use super::jsonc_editor::cloud_code_value;
use super::{
    disable_ide_settings, enable_ide_settings, inspect_ide_settings, IdeSettingsState,
    IDE_CLOUD_CODE_SETTING, IDE_SETTINGS_BACKUP_FILE, IDE_SETTINGS_RECEIPT_FILE,
    IDE_SETTING_OWNERSHIP_FILE,
};
use crate::error::HostIntegrationError;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

const ENDPOINT: &str = "http://127.0.0.1:51234";
const NEXT_ENDPOINT: &str = "http://127.0.0.1:12345";

struct Fixture {
    _temp: TempDir,
    settings_path: PathBuf,
    integration_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self {
            settings_path: temp.path().join("Antigravity IDE/User/settings.json"),
            integration_root: temp.path().join("AGY BYOK/ide-settings"),
            _temp: temp,
        }
    }

    fn write_settings(&self, content: &str) {
        fs::create_dir_all(self.settings_path.parent().unwrap()).unwrap();
        fs::write(&self.settings_path, content).unwrap();
    }
}

fn configured_endpoint(bytes: &[u8], endpoint: &str) -> Result<bool, HostIntegrationError> {
    Ok(cloud_code_value(bytes)?.as_ref().and_then(Value::as_str) == Some(endpoint))
}

#[test]
fn enables_and_disables_when_settings_file_was_absent() {
    let fixture = Fixture::new();
    let enabled =
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(enabled.state, IdeSettingsState::Managed);
    assert!(configured_endpoint(&fs::read(&fixture.settings_path).unwrap(), ENDPOINT).unwrap());

    let disabled =
        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(disabled.state, IdeSettingsState::Disabled);
    assert!(fixture.settings_path.exists());
    assert!(cloud_code_value(&fs::read(&fixture.settings_path).unwrap())
        .unwrap()
        .is_none());
    assert!(!fixture
        .integration_root
        .join(IDE_SETTING_OWNERSHIP_FILE)
        .exists());
}

#[test]
fn jsonc_comments_and_trailing_comma_are_preserved() {
    let fixture = Fixture::new();
    let original = "{\n  // keep this comment\n  \"editor.fontSize\": 15,\n}\n";
    fixture.write_settings(original);

    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let configured = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(configured.contains("// keep this comment"));
    assert!(configured.contains("\"editor.fontSize\": 15"));
    assert!(configured.contains(IDE_CLOUD_CODE_SETTING));

    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let restored = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(restored.contains("// keep this comment"));
    assert!(restored.contains("\"editor.fontSize\": 15,"));
    assert!(!restored.contains(IDE_CLOUD_CODE_SETTING));
}

#[test]
fn existing_endpoint_value_is_replaced_minimally_and_restored() {
    let fixture = Fixture::new();
    let original = "{\n  \"jetski.cloudCodeUrl\": \"https://example.invalid\",\n  \"workbench.colorTheme\": \"Default\"\n}\n";
    fixture.write_settings(original);

    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let configured = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(configured.contains(&format!("\"{ENDPOINT}\"")));
    assert!(configured.contains("\"workbench.colorTheme\": \"Default\""));
    assert_eq!(
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
            .unwrap()
            .state,
        IdeSettingsState::Managed
    );

    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(
        fs::read_to_string(&fixture.settings_path).unwrap(),
        original
    );
}

#[test]
fn repeated_enable_and_disable_are_idempotent() {
    let fixture = Fixture::new();
    fixture.write_settings("{}\n");
    let first =
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let configured = fs::read(&fixture.settings_path).unwrap();
    let second =
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(first, second);
    assert_eq!(fs::read(&fixture.settings_path).unwrap(), configured);

    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let disabled =
        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(disabled.state, IdeSettingsState::Disabled);
}

#[test]
fn matching_endpoint_without_ownership_is_external_and_restorable() {
    let fixture = Fixture::new();
    let original = format!("{{\n  \"jetski.cloudCodeUrl\": \"{ENDPOINT}\"\n}}\n");
    fixture.write_settings(&original);

    let inspected =
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(inspected.state, IdeSettingsState::External);

    let enabled =
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(enabled.state, IdeSettingsState::External);
    assert!(!fixture
        .integration_root
        .join(IDE_SETTING_OWNERSHIP_FILE)
        .exists());

    let disabled =
        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(disabled.state, IdeSettingsState::Disabled);
    assert!(!fs::read_to_string(&fixture.settings_path)
        .unwrap()
        .contains(IDE_CLOUD_CODE_SETTING));
}

#[test]
fn status_only_reads_the_target_value_from_valid_json5() {
    let fixture = Fixture::new();
    fixture.write_settings(&format!(
        "{{\n  unquotedSetting: true,\n  'jetski.cloudCodeUrl': '{ENDPOINT}',\n}}\n"
    ));

    let status =
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

    assert_eq!(status.state, IdeSettingsState::External);
}

#[test]
fn local_endpoint_without_ownership_is_detected_as_external_mismatch() {
    let fixture = Fixture::new();
    let stale_endpoint = "http://127.0.0.1:54321";
    fixture.write_settings(&format!(
        "{{\n  \"jetski.cloudCodeUrl\": \"{stale_endpoint}\"\n}}\n"
    ));

    let status =
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

    assert_eq!(status.state, IdeSettingsState::External);
    assert!(!status.endpoint_matches);
}

#[test]
fn external_local_endpoint_can_be_restored_to_official_settings() {
    let fixture = Fixture::new();
    let stale_endpoint = "http://127.0.0.1:54321";
    fixture.write_settings(&format!(
        "{{\n  \"jetski.cloudCodeUrl\": \"{stale_endpoint}\",\n  \"editor.fontSize\": 14\n}}\n"
    ));

    let restored =
        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

    assert_eq!(restored.state, IdeSettingsState::Disabled);
    let settings = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(!settings.contains(IDE_CLOUD_CODE_SETTING));
    assert!(settings.contains("editor.fontSize"));
}

#[test]
fn unrelated_settings_changes_do_not_create_conflicts() {
    let fixture = Fixture::new();
    fixture.write_settings("{\n  \"editor.fontSize\": 14\n}\n");
    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    fs::write(
        &fixture.settings_path,
        format!("{{\n  \"thirdParty\": true,\n  \"jetski.cloudCodeUrl\": \"{ENDPOINT}\"\n}}\n"),
    )
    .unwrap();

    assert_eq!(
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
            .unwrap()
            .state,
        IdeSettingsState::Managed
    );
    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let restored = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(restored.contains("\"thirdParty\": true"));
    assert!(!restored.contains(IDE_CLOUD_CODE_SETTING));
}

#[test]
fn endpoint_change_preserves_the_value_from_before_first_enable() {
    let fixture = Fixture::new();
    fixture.write_settings(
        "{\n  \"jetski.cloudCodeUrl\": \"https://example.invalid\",\n  \"editor.fontSize\": 14\n}\n",
    );
    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    enable_ide_settings(
        &fixture.settings_path,
        &fixture.integration_root,
        NEXT_ENDPOINT,
    )
    .unwrap();
    assert!(
        configured_endpoint(&fs::read(&fixture.settings_path).unwrap(), NEXT_ENDPOINT).unwrap()
    );

    disable_ide_settings(
        &fixture.settings_path,
        &fixture.integration_root,
        NEXT_ENDPOINT,
    )
    .unwrap();
    assert_eq!(
        cloud_code_value(&fs::read(&fixture.settings_path).unwrap())
            .unwrap()
            .and_then(|value| value.as_str().map(str::to_string))
            .as_deref(),
        Some("https://example.invalid")
    );
}

#[test]
fn stale_managed_endpoint_remains_disableable_after_proxy_port_changes() {
    let fixture = Fixture::new();
    let original =
        "{\n  \"jetski.cloudCodeUrl\": \"https://example.invalid\",\n  \"editor.fontSize\": 14\n}\n";
    fixture.write_settings(original);
    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

    let status = inspect_ide_settings(
        &fixture.settings_path,
        &fixture.integration_root,
        NEXT_ENDPOINT,
    )
    .unwrap();
    assert_eq!(status.state, IdeSettingsState::Managed);
    assert!(!status.endpoint_matches);

    let disabled = disable_ide_settings(
        &fixture.settings_path,
        &fixture.integration_root,
        NEXT_ENDPOINT,
    )
    .unwrap();
    assert_eq!(disabled.state, IdeSettingsState::Disabled);
    assert_eq!(
        fs::read_to_string(&fixture.settings_path).unwrap(),
        original
    );
    assert!(!fixture
        .integration_root
        .join(IDE_SETTING_OWNERSHIP_FILE)
        .exists());
}

#[test]
fn user_endpoint_change_is_treated_as_disabled_and_is_not_overwritten_on_disable() {
    let fixture = Fixture::new();
    fixture.write_settings("{}\n");
    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    fixture.write_settings(
        "{\n  \"jetski.cloudCodeUrl\": \"http://127.0.0.1:60000\",\n  \"userSetting\": true\n}\n",
    );

    assert_eq!(
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
            .unwrap()
            .state,
        IdeSettingsState::Disabled
    );
    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    let unchanged = fs::read_to_string(&fixture.settings_path).unwrap();
    assert!(unchanged.contains("http://127.0.0.1:60000"));
    assert!(unchanged.contains("\"userSetting\": true"));
}

#[test]
fn legacy_receipt_and_backup_do_not_affect_status() {
    let fixture = Fixture::new();
    fixture.write_settings("{}\n");
    fs::create_dir_all(&fixture.integration_root).unwrap();
    fs::write(
        fixture.integration_root.join(IDE_SETTINGS_RECEIPT_FILE),
        b"legacy receipt",
    )
    .unwrap();
    fs::write(
        fixture.integration_root.join(IDE_SETTINGS_BACKUP_FILE),
        b"legacy backup",
    )
    .unwrap();

    assert_eq!(
        inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
            .unwrap()
            .state,
        IdeSettingsState::Disabled
    );
}

#[cfg(unix)]
#[test]
fn settings_permissions_are_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fixture.write_settings("{}\n");
    fs::set_permissions(&fixture.settings_path, fs::Permissions::from_mode(0o640)).unwrap();

    enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(
        fs::metadata(&fixture.settings_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
    assert_eq!(
        fs::metadata(&fixture.settings_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn symlink_settings_path_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let target = fixture.settings_path.with_file_name("real-settings.json");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "{}\n").unwrap();
    symlink(&target, &fixture.settings_path).unwrap();

    assert!(matches!(
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT),
        Err(HostIntegrationError::SettingsConflict(_))
    ));
}
