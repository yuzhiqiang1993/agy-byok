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
const RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum IdeSettingsReceiptState {
    Prepared,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IdeSettingsReceipt {
    schema_version: u32,
    state: IdeSettingsReceiptState,
    settings_path: PathBuf,
    endpoint: String,
    original_existed: bool,
    original_sha256: String,
    configured_sha256: String,
    backup_relative_path: PathBuf,
    created_at_unix_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdeSettingsState {
    Disabled,
    Enabled,
    External,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IdeSettingsStatus {
    pub state: IdeSettingsState,
    pub settings_path: PathBuf,
    pub receipt_path: Option<PathBuf>,
}

pub fn inspect_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    validate_integration_root_if_present(integration_root)?;
    let receipt_path = integration_root.join(IDE_SETTINGS_RECEIPT_FILE);
    let backup_path = integration_root.join(IDE_SETTINGS_BACKUP_FILE);

    if !receipt_path.is_file() {
        if receipt_path.exists()
            || receipt_path.is_symlink()
            || backup_path.exists()
            || backup_path.is_symlink()
        {
            return Err(settings_conflict(
                "orphaned IDE settings receipt or backup requires manual inspection",
            ));
        }
        let state = match read_optional_regular_file(&settings_path)? {
            Some(bytes) if configured_endpoint(&bytes, endpoint)? => IdeSettingsState::External,
            _ => IdeSettingsState::Disabled,
        };
        return Ok(IdeSettingsStatus {
            state,
            settings_path,
            receipt_path: None,
        });
    }

    let receipt = read_and_validate_receipt(&receipt_path, &backup_path, &settings_path, endpoint)?;
    let current = read_optional_regular_file(&settings_path)?;
    let current_sha256 = hash_optional(current.as_deref());
    let state = if current.is_some() && current_sha256 == receipt.configured_sha256 {
        IdeSettingsState::Enabled
    } else if matches!(receipt.state, IdeSettingsReceiptState::Prepared)
        && current_sha256 == receipt.original_sha256
        && current.is_some() == receipt.original_existed
    {
        IdeSettingsState::Disabled
    } else {
        return Err(settings_conflict(
            "IDE settings changed after AGY BYOK activation; automatic overwrite is blocked",
        ));
    };

    Ok(IdeSettingsStatus {
        state,
        settings_path,
        receipt_path: Some(receipt_path),
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
    let receipt_path = integration_root.join(IDE_SETTINGS_RECEIPT_FILE);
    let backup_path = integration_root.join(IDE_SETTINGS_BACKUP_FILE);

    if receipt_path.is_file() {
        let mut receipt =
            read_and_validate_receipt(&receipt_path, &backup_path, &settings_path, endpoint)?;
        let current = read_optional_regular_file(&settings_path)?;
        let current_sha256 = hash_optional(current.as_deref());
        if current.is_some() && current_sha256 == receipt.configured_sha256 {
            if receipt.state != IdeSettingsReceiptState::Active {
                receipt.state = IdeSettingsReceiptState::Active;
                write_json_private(&receipt_path, &receipt)?;
            }
            return Ok(enabled_status(settings_path, receipt_path));
        }
        if matches!(receipt.state, IdeSettingsReceiptState::Prepared)
            && current_sha256 == receipt.original_sha256
            && current.is_some() == receipt.original_existed
        {
            let backup = read_regular_file(&backup_path)?;
            let configured = configure_settings(&backup, endpoint)?;
            if crate::sha256(&configured) != receipt.configured_sha256 {
                return Err(settings_conflict(
                    "prepared IDE settings candidate no longer matches its receipt",
                ));
            }
            write_settings_file(&settings_path, &configured)?;
            receipt.state = IdeSettingsReceiptState::Active;
            write_json_private(&receipt_path, &receipt)?;
            return Ok(enabled_status(settings_path, receipt_path));
        }
        return Err(settings_conflict(
            "IDE settings changed after AGY BYOK prepared activation; automatic overwrite is blocked",
        ));
    }
    if receipt_path.exists()
        || receipt_path.is_symlink()
        || backup_path.exists()
        || backup_path.is_symlink()
    {
        return Err(settings_conflict(
            "orphaned IDE settings receipt or backup requires manual inspection",
        ));
    }

    let original = read_optional_regular_file(&settings_path)?;
    if let Some(bytes) = original.as_deref() {
        if configured_endpoint(bytes, endpoint)? {
            return Ok(IdeSettingsStatus {
                state: IdeSettingsState::External,
                settings_path,
                receipt_path: None,
            });
        }
    }
    let original_existed = original.is_some();
    let original_bytes = original.unwrap_or_else(|| b"{}\n".to_vec());
    let configured = configure_settings(&original_bytes, endpoint)?;
    let mut receipt = IdeSettingsReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        state: IdeSettingsReceiptState::Prepared,
        settings_path: settings_path.clone(),
        endpoint: endpoint.to_string(),
        original_existed,
        original_sha256: if original_existed {
            crate::sha256(&original_bytes)
        } else {
            crate::sha256(&[])
        },
        configured_sha256: crate::sha256(&configured),
        backup_relative_path: PathBuf::from(IDE_SETTINGS_BACKUP_FILE),
        created_at_unix_ms: unix_time_ms(),
    };

    write_private_file(
        &backup_path,
        if original_existed {
            &original_bytes
        } else {
            &[]
        },
    )?;
    if let Err(error) = write_json_private(&receipt_path, &receipt) {
        remove_file_if_present(&backup_path);
        return Err(error);
    }
    write_settings_file(&settings_path, &configured)?;
    receipt.state = IdeSettingsReceiptState::Active;
    write_json_private(&receipt_path, &receipt)?;
    Ok(enabled_status(settings_path, receipt_path))
}

pub fn disable_ide_settings(
    settings_path: impl AsRef<Path>,
    integration_root: impl AsRef<Path>,
    endpoint: &str,
) -> Result<IdeSettingsStatus, HostIntegrationError> {
    let settings_path = validate_settings_path(settings_path.as_ref())?;
    let integration_root = integration_root.as_ref();
    validate_integration_root_if_present(integration_root)?;
    let receipt_path = integration_root.join(IDE_SETTINGS_RECEIPT_FILE);
    let backup_path = integration_root.join(IDE_SETTINGS_BACKUP_FILE);

    if !receipt_path.is_file() {
        if receipt_path.exists()
            || receipt_path.is_symlink()
            || backup_path.exists()
            || backup_path.is_symlink()
        {
            return Err(settings_conflict(
                "orphaned IDE settings receipt or backup requires manual inspection",
            ));
        }
        return inspect_ide_settings(&settings_path, integration_root, endpoint);
    }

    let receipt = read_and_validate_receipt(&receipt_path, &backup_path, &settings_path, endpoint)?;
    let current = read_optional_regular_file(&settings_path)?;
    let current_sha256 = hash_optional(current.as_deref());
    let is_configured = current.is_some() && current_sha256 == receipt.configured_sha256;
    let is_already_restored =
        current_sha256 == receipt.original_sha256 && current.is_some() == receipt.original_existed;
    if !is_configured && !is_already_restored {
        return Err(settings_conflict(
            "IDE settings changed after AGY BYOK activation; automatic restore is blocked",
        ));
    }

    if is_configured {
        if receipt.original_existed {
            let original = read_regular_file(&backup_path)?;
            write_settings_file(&settings_path, &original)?;
        } else {
            remove_regular_file(&settings_path)?;
        }
    }

    remove_regular_file(&backup_path)?;
    remove_regular_file(&receipt_path)?;
    Ok(IdeSettingsStatus {
        state: IdeSettingsState::Disabled,
        settings_path,
        receipt_path: None,
    })
}

fn enabled_status(settings_path: PathBuf, receipt_path: PathBuf) -> IdeSettingsStatus {
    IdeSettingsStatus {
        state: IdeSettingsState::Enabled,
        settings_path,
        receipt_path: Some(receipt_path),
    }
}

fn read_and_validate_receipt(
    receipt_path: &Path,
    backup_path: &Path,
    settings_path: &Path,
    endpoint: &str,
) -> Result<IdeSettingsReceipt, HostIntegrationError> {
    let bytes = read_regular_file(receipt_path)?;
    let receipt: IdeSettingsReceipt =
        serde_json::from_slice(&bytes).map_err(|source| HostIntegrationError::Json {
            path: receipt_path.to_path_buf(),
            source,
        })?;
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.settings_path != settings_path
        || receipt.endpoint != endpoint
        || receipt.backup_relative_path != Path::new(IDE_SETTINGS_BACKUP_FILE)
    {
        return Err(settings_conflict(
            "IDE settings receipt does not match the requested integration",
        ));
    }
    let backup = read_regular_file(backup_path)?;
    let expected_backup_hash = if receipt.original_existed {
        crate::sha256(&backup)
    } else if backup.is_empty() {
        crate::sha256(&[])
    } else {
        return Err(settings_conflict(
            "IDE settings backup should be empty for an originally absent file",
        ));
    };
    if expected_backup_hash != receipt.original_sha256 {
        return Err(settings_conflict(
            "IDE settings backup hash does not match its receipt",
        ));
    }
    Ok(receipt)
}

fn configured_endpoint(bytes: &[u8], endpoint: &str) -> Result<bool, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| settings_conflict("IDE settings root must be an object"))?;
    Ok(object.get(IDE_CLOUD_CODE_SETTING).and_then(Value::as_str) == Some(endpoint))
}

fn configure_settings(bytes: &[u8], endpoint: &str) -> Result<Vec<u8>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    if !value.is_object() {
        return Err(settings_conflict("IDE settings root must be an object"));
    }

    let object = scan_root_object(source)?;
    let matches = object
        .properties
        .iter()
        .filter(|property| property.key == IDE_CLOUD_CODE_SETTING)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(settings_conflict(
            "IDE settings contains duplicate jetski.cloudCodeUrl keys",
        ));
    }
    let encoded_endpoint =
        serde_json::to_string(endpoint).expect("string serialization cannot fail");
    let configured = if let Some(property) = matches.first() {
        format!(
            "{}{}{}",
            &source[..property.value_start],
            encoded_endpoint,
            &source[property.value_end..]
        )
    } else {
        let insertion = if object.properties.is_empty() || object.trailing_comma {
            format!(
                "\n  {}: {}\n",
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_endpoint
            )
        } else {
            format!(
                ",\n  {}: {}\n",
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_endpoint
            )
        };
        format!(
            "{}{}{}",
            &source[..object.close_brace],
            insertion,
            &source[object.close_brace..]
        )
    };
    if !configured_endpoint(configured.as_bytes(), endpoint)? {
        return Err(settings_conflict(
            "configured IDE settings did not retain the requested endpoint",
        ));
    }
    Ok(configured.into_bytes())
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
    value_start: usize,
    value_end: usize,
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
        properties.push(JsonProperty {
            key,
            value_start,
            value_end,
        });
        index = value_end;
        skip_trivia(bytes, &mut index)?;
        match bytes.get(index) {
            Some(b',') => {
                index += 1;
                skip_trivia(bytes, &mut index)?;
                trailing_comma = bytes.get(index) == Some(&b'}');
                if !trailing_comma {
                    continue;
                }
            }
            Some(b'}') => {
                trailing_comma = false;
            }
            _ => {
                return Err(settings_conflict(
                    "IDE settings property is missing a comma or closing brace",
                ))
            }
        }
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

fn hash_optional(bytes: Option<&[u8]>) -> String {
    crate::sha256(bytes.unwrap_or(&[]))
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

    const ENDPOINT: &str = "http://127.0.0.1:50999";

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
        assert!(!fixture.settings_path.exists());
        assert!(!fixture
            .integration_root
            .join(IDE_SETTINGS_RECEIPT_FILE)
            .exists());
        assert!(!fixture
            .integration_root
            .join(IDE_SETTINGS_BACKUP_FILE)
            .exists());
    }

    #[test]
    fn jsonc_comments_and_trailing_comma_are_preserved_and_restored_byte_for_byte() {
        let fixture = Fixture::new();
        let original = "{\n  // keep this comment\n  \"editor.fontSize\": 15,\n}\n";
        fixture.write_settings(original);

        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
        let configured = fs::read_to_string(&fixture.settings_path).unwrap();
        assert!(configured.contains("// keep this comment"));
        assert!(configured.contains("\"editor.fontSize\": 15"));
        assert!(configured.contains(IDE_CLOUD_CODE_SETTING));

        disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
        assert_eq!(
            fs::read_to_string(&fixture.settings_path).unwrap(),
            original
        );
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
    fn externally_configured_endpoint_is_not_claimed_or_removed() {
        let fixture = Fixture::new();
        fixture.write_settings(&format!(
            "{{\n  \"jetski.cloudCodeUrl\": \"{ENDPOINT}\"\n}}\n"
        ));
        let original = fs::read(&fixture.settings_path).unwrap();

        let status =
            enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(status.state, IdeSettingsState::External);
        assert!(status.receipt_path.is_none());
        assert_eq!(fs::read(&fixture.settings_path).unwrap(), original);

        let status =
            disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(status.state, IdeSettingsState::External);
        assert_eq!(fs::read(&fixture.settings_path).unwrap(), original);
    }

    #[test]
    fn settings_drift_blocks_automatic_restore() {
        let fixture = Fixture::new();
        fixture.write_settings("{\n  \"editor.fontSize\": 14\n}\n");
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
        fs::write(&fixture.settings_path, "{\n  \"thirdParty\": true\n}\n").unwrap();

        let error =
            disable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap_err();
        assert!(matches!(error, HostIntegrationError::SettingsConflict(_)));
        assert!(fixture
            .integration_root
            .join(IDE_SETTINGS_RECEIPT_FILE)
            .exists());
    }

    #[test]
    fn prepared_receipt_with_original_settings_can_resume_activation() {
        let fixture = Fixture::new();
        let original = b"{\n  \"editor.fontSize\": 14\n}\n";
        fixture.write_settings(std::str::from_utf8(original).unwrap());
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

        let receipt_path = fixture.integration_root.join(IDE_SETTINGS_RECEIPT_FILE);
        let backup_path = fixture.integration_root.join(IDE_SETTINGS_BACKUP_FILE);
        let mut receipt = read_and_validate_receipt(
            &receipt_path,
            &backup_path,
            &fixture.settings_path,
            ENDPOINT,
        )
        .unwrap();
        receipt.state = IdeSettingsReceiptState::Prepared;
        write_json_private(&receipt_path, &receipt).unwrap();
        write_settings_file(&fixture.settings_path, original).unwrap();

        let interrupted =
            inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(interrupted.state, IdeSettingsState::Disabled);
        assert_eq!(
            interrupted.receipt_path.as_deref(),
            Some(receipt_path.as_path())
        );

        let resumed =
            enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap();
        assert_eq!(resumed.state, IdeSettingsState::Enabled);
        assert!(configured_endpoint(&fs::read(&fixture.settings_path).unwrap(), ENDPOINT).unwrap());
    }

    #[test]
    fn prepared_receipt_with_configured_settings_is_finalized_by_enable() {
        let fixture = Fixture::new();
        fixture.write_settings("{}\n");
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();

        let receipt_path = fixture.integration_root.join(IDE_SETTINGS_RECEIPT_FILE);
        let backup_path = fixture.integration_root.join(IDE_SETTINGS_BACKUP_FILE);
        let mut receipt = read_and_validate_receipt(
            &receipt_path,
            &backup_path,
            &fixture.settings_path,
            ENDPOINT,
        )
        .unwrap();
        receipt.state = IdeSettingsReceiptState::Prepared;
        write_json_private(&receipt_path, &receipt).unwrap();

        assert_eq!(
            inspect_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT)
                .unwrap()
                .state,
            IdeSettingsState::Enabled
        );
        enable_ide_settings(&fixture.settings_path, &fixture.integration_root, ENDPOINT).unwrap();
        let finalized = read_and_validate_receipt(
            &receipt_path,
            &backup_path,
            &fixture.settings_path,
            ENDPOINT,
        )
        .unwrap();
        assert_eq!(finalized.state, IdeSettingsReceiptState::Active);
    }

    #[test]
    fn tampered_backup_or_receipt_is_rejected() {
        let backup_fixture = Fixture::new();
        backup_fixture.write_settings("{}\n");
        enable_ide_settings(
            &backup_fixture.settings_path,
            &backup_fixture.integration_root,
            ENDPOINT,
        )
        .unwrap();
        fs::write(
            backup_fixture
                .integration_root
                .join(IDE_SETTINGS_BACKUP_FILE),
            b"tampered",
        )
        .unwrap();
        assert!(matches!(
            inspect_ide_settings(
                &backup_fixture.settings_path,
                &backup_fixture.integration_root,
                ENDPOINT
            ),
            Err(HostIntegrationError::SettingsConflict(_))
        ));

        let receipt_fixture = Fixture::new();
        receipt_fixture.write_settings("{}\n");
        enable_ide_settings(
            &receipt_fixture.settings_path,
            &receipt_fixture.integration_root,
            ENDPOINT,
        )
        .unwrap();
        let receipt_path = receipt_fixture
            .integration_root
            .join(IDE_SETTINGS_RECEIPT_FILE);
        let backup_path = receipt_fixture
            .integration_root
            .join(IDE_SETTINGS_BACKUP_FILE);
        let mut receipt = read_and_validate_receipt(
            &receipt_path,
            &backup_path,
            &receipt_fixture.settings_path,
            ENDPOINT,
        )
        .unwrap();
        receipt.endpoint = "https://tampered.invalid".to_string();
        write_json_private(&receipt_path, &receipt).unwrap();
        assert!(matches!(
            inspect_ide_settings(
                &receipt_fixture.settings_path,
                &receipt_fixture.integration_root,
                ENDPOINT
            ),
            Err(HostIntegrationError::SettingsConflict(_))
        ));
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
