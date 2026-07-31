use crate::error::{io_error, HostIntegrationError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const IDE_CLOUD_CODE_SETTING: &str = "jetski.cloudCodeUrl";
pub const IDE_SETTINGS_RECEIPT_FILE: &str = "ide-settings-receipt.json";
pub const IDE_SETTINGS_BACKUP_FILE: &str = "ide-settings-original.jsonc";
pub const IDE_SETTING_OWNERSHIP_FILE: &str = "ide-setting-ownership.json";
const OWNERSHIP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct IdeSettingOwnership {
    schema_version: u32,
    settings_path: PathBuf,
    managed_endpoint: String,
    previous_value: Option<Value>,
    previous_trailing_comma: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdeSettingsState {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IdeSettingsStatus {
    pub state: IdeSettingsState,
    pub settings_path: PathBuf,
}

pub fn inspect_ide_settings(
    settings_path: impl AsRef<Path>,
    _integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = validate_settings_path(settings_path.as_ref())?;
    let state = match read_optional_regular_file(&settings_path)? {
        Some(bytes) if configured_endpoint(&bytes, endpoint)? => IdeSettingsState::Enabled,
        _ => IdeSettingsState::Disabled,
    };
    Ok(IdeSettingsStatus {
        state,
        settings_path,
    })
}

pub fn enable_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    prepare_private_directory(integration_root)?;
    let current = read_optional_regular_file(&settings_path)?;
    let current_bytes = current.unwrap_or_else(|| b"{}\n".to_vec());
    let current_value = cloud_code_value(&current_bytes)?;
    let current_trailing_comma = settings_root_object(&current_bytes)?.trailing_comma;
    if current_value.as_ref().and_then(Value::as_str) == Some(endpoint) {
        return Ok(enabled_status(settings_path));
    }

    let ownership_path = integration_root.join(IDE_SETTING_OWNERSHIP_FILE);
    let previous_ownership = read_ownership_if_present(&ownership_path, &settings_path)?;
    let (previous_value, previous_trailing_comma) = match previous_ownership {
        Some(ownership)
            if current_value.as_ref().and_then(Value::as_str)
                == Some(ownership.managed_endpoint.as_str()) =>
        {
            (ownership.previous_value, ownership.previous_trailing_comma)
        }
        _ => (current_value, current_trailing_comma),
    };
    let ownership = IdeSettingOwnership {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        settings_path: settings_path.clone(),
        managed_endpoint: endpoint.to_string(),
        previous_value,
        previous_trailing_comma,
    };
    write_json_private(&ownership_path, &ownership)?;
    let configured = configure_settings(&current_bytes, endpoint)?;
    write_settings_file(&settings_path, &configured)?;
    Ok(enabled_status(settings_path))
}

pub fn disable_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    validate_integration_root_if_present(integration_root)?;
    let Some(current) = read_optional_regular_file(&settings_path)? else {
        return Ok(disabled_status(settings_path));
    };
    let current_value = cloud_code_value(&current)?;
    if current_value.as_ref().and_then(Value::as_str) != Some(endpoint) {
        return Ok(disabled_status(settings_path));
    }

    let ownership_path = integration_root.join(IDE_SETTING_OWNERSHIP_FILE);
    let ownership = read_ownership_if_present(&ownership_path, &settings_path)?;
    let previous_value = ownership
        .as_ref()
        .filter(|ownership| ownership.managed_endpoint == endpoint)
        .and_then(|ownership| ownership.previous_value.as_ref());
    let updated = match previous_value {
        Some(previous_value) => configure_setting_value(&current, previous_value)?,
        None => remove_setting(
            &current,
            ownership
                .as_ref()
                .filter(|ownership| ownership.managed_endpoint == endpoint)
                .is_some_and(|ownership| ownership.previous_trailing_comma),
        )?,
    };
    write_settings_file(&settings_path, &updated)?;
    if ownership_path.exists() || ownership_path.is_symlink() {
        remove_regular_file(&ownership_path)?;
    }
    Ok(disabled_status(settings_path))
}

fn enabled_status(settings_path: PathBuf) -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::Enabled,
        settings_path,
    }
}

fn disabled_status(settings_path: PathBuf) -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::Disabled,
        settings_path,
    }
}

fn read_ownership_if_present(
    ownership_path: &Path,
    settings_path: &Path,
) -> Result<Option<IdeSettingOwnership>, HostIntegrationError> {
    if !ownership_path.exists() && !ownership_path.is_symlink() {
        return Ok(None);
    }
    let bytes = read_regular_file(ownership_path)?;
    let ownership: IdeSettingOwnership =
        serde_json::from_slice(&bytes).map_err(|source| HostIntegrationError::Json {
            path: ownership_path.to_path_buf(),
            source,
        })?;
    if ownership.schema_version != OWNERSHIP_SCHEMA_VERSION
        || ownership.settings_path != settings_path
    {
        return Err(settings_conflict(
            "IDE setting ownership does not match the requested settings file",
        ));
    }
    Ok(Some(ownership))
}

fn configured_endpoint(bytes: &[u8], endpoint: &str) -> Result<bool, HostIntegrationError> {
    Ok(cloud_code_value(bytes)?.as_ref().and_then(Value::as_str) == Some(endpoint))
}

fn cloud_code_value(bytes: &[u8]) -> Result<Option<Value>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| settings_conflict("IDE settings root must be an object"))?;
    Ok(object.get(IDE_CLOUD_CODE_SETTING).cloned())
}

fn settings_root_object(bytes: &[u8]) -> Result<RootObject, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    ensure_unique_cloud_code_property(source)
}

fn ensure_unique_cloud_code_property(source: &str) -> Result<RootObject, HostIntegrationError> {
    let object = scan_root_object(source)?;
    if object
        .properties
        .iter()
        .filter(|property| property.key == IDE_CLOUD_CODE_SETTING)
        .count()
        > 1
    {
        return Err(settings_conflict(
            "IDE settings contains duplicate jetski.cloudCodeUrl keys",
        ));
    }
    Ok(object)
}

fn configure_settings(bytes: &[u8], endpoint: &str) -> Result<Vec<u8>, HostIntegrationError> {
    configure_setting_value(bytes, &Value::String(endpoint.to_string()))
}

fn configure_setting_value(
    bytes: &[u8],
    configured_value: &Value,
) -> Result<Vec<u8>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    if !value.is_object() {
        return Err(settings_conflict("IDE settings root must be an object"));
    }

    let object = ensure_unique_cloud_code_property(source)?;
    let matches = object
        .properties
        .iter()
        .filter(|property| property.key == IDE_CLOUD_CODE_SETTING)
        .collect::<Vec<_>>();
    let encoded_value =
        serde_json::to_string(configured_value).expect("JSON value serialization cannot fail");
    let configured = if let Some(property) = matches.first() {
        format!(
            "{}{}{}",
            &source[..property.value_start],
            encoded_value,
            &source[property.value_end..]
        )
    } else {
        let insertion = if object.properties.is_empty() || object.trailing_comma {
            format!(
                "\n  {}: {}\n",
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_value
            )
        } else {
            format!(
                ",\n  {}: {}\n",
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_value
            )
        };
        format!(
            "{}{}{}",
            &source[..object.close_brace],
            insertion,
            &source[object.close_brace..]
        )
    };
    if cloud_code_value(configured.as_bytes())?.as_ref() != Some(configured_value) {
        return Err(settings_conflict(
            "configured IDE settings did not retain the requested value",
        ));
    }
    Ok(configured.into_bytes())
}

fn remove_setting(
    bytes: &[u8],
    retain_preceding_comma: bool,
) -> Result<Vec<u8>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    if !value.is_object() {
        return Err(settings_conflict("IDE settings root must be an object"));
    }
    let object = ensure_unique_cloud_code_property(source)?;
    let Some(property) = object
        .properties
        .iter()
        .find(|property| property.key == IDE_CLOUD_CODE_SETTING)
    else {
        return Ok(bytes.to_vec());
    };

    let (remove_start, remove_end) = match (property.comma_before, property.comma_after) {
        (_, Some(comma_after)) => (property.property_start, comma_after + 1),
        (Some(_), None) if retain_preceding_comma => (property.property_start, property.value_end),
        (Some(comma_before), None) => (comma_before, property.value_end),
        (None, None) => (property.property_start, property.value_end),
    };
    let updated = format!("{}{}", &source[..remove_start], &source[remove_end..]);
    if cloud_code_value(updated.as_bytes())?.is_some() {
        return Err(settings_conflict(
            "updated IDE settings still contains jetski.cloudCodeUrl",
        ));
    }
    Ok(updated.into_bytes())
}

#[derive(Debug)]
struct RootObject {
    close_brace: usize,
    properties: Vec<JsonProperty>,
    trailing_comma: bool,
}

#[derive(Debug)]
struct JsonProperty {
    key: String,
    property_start: usize,
    value_start: usize,
    value_end: usize,
    comma_before: Option<usize>,
    comma_after: Option<usize>,
}

fn scan_root_object(source: &str) -> Result<RootObject, HostIntegrationError> {
    let bytes = source.as_bytes();
    let mut index = 0;
    skip_trivia(bytes, &mut index)?;
    if bytes.get(index) != Some(&b'{') {
        return Err(settings_conflict(
            "IDE settings root must start with an object",
        ));
    }
    index += 1;
    let mut properties = Vec::new();
    let mut trailing_comma = false;
    let mut comma_before = None;

    loop {
        skip_trivia(bytes, &mut index)?;
        if bytes.get(index) == Some(&b'}') {
            return Ok(RootObject {
                close_brace: index,
                properties,
                trailing_comma,
            });
        }
        let key_start = index;
        let key_end = parse_string_end(bytes, index)?;
        let key: String = serde_json::from_str(&source[key_start..key_end]).map_err(|error| {
            settings_conflict(format!("invalid quoted IDE settings key: {error}"))
        })?;
        index = key_end;
        skip_trivia(bytes, &mut index)?;
        if bytes.get(index) != Some(&b':') {
            return Err(settings_conflict("IDE settings property is missing ':'"));
        }
        index += 1;
        skip_trivia(bytes, &mut index)?;
        let value_start = index;
        let value_end = parse_value_end(bytes, index)?;
        index = value_end;
        skip_trivia(bytes, &mut index)?;
        let comma_after = match bytes.get(index) {
            Some(b',') => {
                let comma_after = index;
                index += 1;
                skip_trivia(bytes, &mut index)?;
                trailing_comma = bytes.get(index) == Some(&b'}');
                Some(comma_after)
            }
            Some(b'}') => {
                trailing_comma = false;
                None
            }
            _ => {
                return Err(settings_conflict(
                    "IDE settings property is missing a comma or closing brace",
                ))
            }
        };
        properties.push(JsonProperty {
            key,
            property_start: key_start,
            value_start,
            value_end,
            comma_before,
            comma_after,
        });
        if trailing_comma || comma_after.is_none() {
            continue;
        }
        comma_before = comma_after;
    }
}

fn parse_value_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    match bytes.get(start) {
        Some(b'"') => parse_string_end(bytes, start),
        Some(b'{') | Some(b'[') => parse_composite_end(bytes, start),
        Some(_) => {
            let mut index = start;
            while let Some(byte) = bytes.get(index) {
                if byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']') {
                    break;
                }
                if *byte == b'/' && matches!(bytes.get(index + 1), Some(b'/') | Some(b'*')) {
                    break;
                }
                index += 1;
            }
            if index == start {
                Err(settings_conflict(
                    "IDE settings property has an empty value",
                ))
            } else {
                Ok(index)
            }
        }
        None => Err(settings_conflict(
            "IDE settings property has an empty value",
        )),
    }
}

fn parse_composite_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    let mut stack = vec![if bytes[start] == b'{' { b'}' } else { b']' }];
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'"' => index = parse_string_end(bytes, index)?,
            b'/' if bytes.get(index + 1) == Some(&b'/') => skip_line_comment(bytes, &mut index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => skip_block_comment(bytes, &mut index)?,
            b'{' => {
                stack.push(b'}');
                index += 1;
            }
            b'[' => {
                stack.push(b']');
                index += 1;
            }
            b'}' | b']' => {
                if stack.pop() != Some(byte) {
                    return Err(settings_conflict(
                        "IDE settings contains mismatched brackets",
                    ));
                }
                index += 1;
                if stack.is_empty() {
                    return Ok(index);
                }
            }
            _ => index += 1,
        }
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated composite value",
    ))
}

fn parse_string_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    if bytes.get(start) != Some(&b'"') {
        return Err(settings_conflict(
            "IDE settings property keys must be quoted",
        ));
    }
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'\\' => {
                index += 2;
                if index > bytes.len() {
                    return Err(settings_conflict("IDE settings contains an invalid escape"));
                }
            }
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated string",
    ))
}

fn skip_trivia(bytes: &[u8], index: &mut usize) -> Result<(), HostIntegrationError> {
    loop {
        while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
            *index += 1;
        }
        if bytes.get(*index) == Some(&b'/') && bytes.get(*index + 1) == Some(&b'/') {
            skip_line_comment(bytes, index);
        } else if bytes.get(*index) == Some(&b'/') && bytes.get(*index + 1) == Some(&b'*') {
            skip_block_comment(bytes, index)?;
        } else {
            return Ok(());
        }
    }
}

fn skip_line_comment(bytes: &[u8], index: &mut usize) {
    *index += 2;
    while let Some(byte) = bytes.get(*index) {
        *index += 1;
        if *byte == b'\n' {
            break;
        }
    }
}

fn skip_block_comment(bytes: &[u8], index: &mut usize) -> Result<(), HostIntegrationError> {
    *index += 2;
    while *index + 1 < bytes.len() {
        if bytes[*index] == b'*' && bytes[*index + 1] == b'/' {
            *index += 2;
            return Ok(());
        }
        *index += 1;
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated block comment",
    ))
}

fn validate_settings_path(path: &Path) -> Result<PathBuf, HostIntegrationError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
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

fn validate_integration_root_if_present(path: &Path) -> Result<(), HostIntegrationError> {
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

fn prepare_private_directory(path: &Path) -> Result<(), HostIntegrationError> {
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

fn read_optional_regular_file(path: &Path) -> Result<Option<Vec<u8>>, HostIntegrationError> {
    if !path.exists() && !path.is_symlink() {
        return Ok(None);
    }
    read_regular_file(path).map(Some)
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(settings_conflict(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    fs::read(path).map_err(|error| io_error(path, error))
}

fn write_settings_file(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| settings_conflict("IDE settings path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    if path.is_symlink() {
        return Err(settings_conflict("IDE settings path must not be a symlink"));
    }
    let permissions = if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(settings_conflict(
                "IDE settings path must be a regular file",
            ));
        }
        Some(metadata.permissions())
    } else {
        None
    };
    atomic_write(path, bytes, permissions.as_ref())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), HostIntegrationError> {
    atomic_write(path, bytes, None)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(path, error))?;
    }
    Ok(())
}

fn write_json_private<T: Serialize>(path: &Path, value: &T) -> Result<(), HostIntegrationError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    write_private_file(path, &bytes)
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<(), HostIntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| settings_conflict("target path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temporary = parent.join(format!(
        ".agy-byok-settings-{}-{}.next",
        std::process::id(),
        unix_time_ms()
    ));
    if temporary.exists() || temporary.is_symlink() {
        return Err(settings_conflict(
            "temporary IDE settings path already exists",
        ));
    }
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
        fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        remove_file_if_present(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), HostIntegrationError> {
    let directory = fs::File::open(path).map_err(|error| io_error(path, error))?;
    directory.sync_all().map_err(|error| io_error(path, error))
}

fn remove_regular_file(path: &Path) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(settings_conflict(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    fs::remove_file(path).map_err(|error| io_error(path, error))?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) {
    if path.is_file() && !path.is_symlink() {
        let _ = fs::remove_file(path);
    }
}

fn settings_conflict(message: impl Into<String>) -> HostIntegrationError {
    HostIntegrationError::SettingsConflict(message.into())
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
    use tempfile::TempDir;

    const ENDPOINT: &str = "http://127.0.0.1:51234";
    const NEXT_ENDPOINT: &str = "http://127.0.0.1:54321";

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

    #[test]
    fn enables_and_disables_when_settings_file_was_absent() {
        let fixture = Fixture::new();
        let enabled =
            enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(enabled.state, IdeSettingsState::Enabled);
        assert!(configured_endpoint(&fs::read(&fixture.settings_path).unwrap(), ENDPOINT).unwrap());

        let disabled =
            disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
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
            enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        let configured = fs::read(&fixture.settings_path).unwrap();
        let second =
            enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::read(&fixture.settings_path).unwrap(), configured);

        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
        let disabled =
            disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(disabled.state, IdeSettingsState::Disabled);
    }

    #[test]
    fn matching_endpoint_is_enabled_without_ownership() {
        let fixture = Fixture::new();
        fixture.write_settings(&format!(
            "{{\n  \"jetski.cloudCodeUrl\": \"{ENDPOINT}\"\n}}\n"
        ));

        let status =
            inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(status.state, IdeSettingsState::Enabled);

        let status =
            disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(status.state, IdeSettingsState::Disabled);
        assert!(cloud_code_value(&fs::read(&fixture.settings_path).unwrap())
            .unwrap()
            .is_none());
    }

    #[test]
    fn status_only_reads_the_target_value_from_valid_json5() {
        let fixture = Fixture::new();
        fixture.write_settings(&format!(
            "{{\n  unquotedSetting: true,\n  'jetski.cloudCodeUrl': '{ENDPOINT}',\n}}\n"
        ));

        let status =
            inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();

        assert_eq!(status.state, IdeSettingsState::Enabled);
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
            IdeSettingsState::Enabled
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
}
