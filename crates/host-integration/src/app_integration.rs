use crate::error::{io_error, HostIntegrationError};
use crate::sha256;
use plist::Value as PlistValue;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_ANTIGRAVITY_APP_PATH: &str = "/Applications/Antigravity.app";
pub const TARGET_OFFICIAL_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";

const APP_INTEGRATION_SCHEMA_VERSION: u32 = 1;
const RECEIPT_FILE_NAME: &str = ".agy-byok-language-server.json";
const WRAPPER_MARKER: &str = "# AGY-BYOK-MANAGED-LANGUAGE-SERVER v1";
const ENDPOINT_MARKER: &str = "# AGY-BYOK-ENDPOINT: ";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppIntegrationState {
    Disabled,
    Managed,
    Mismatch,
    Conflict,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppIntegrationStatus {
    pub state: AppIntegrationState,
    pub app_path: PathBuf,
    pub endpoint_matches: bool,
    pub configured_endpoint: Option<String>,
    pub app_version: Option<String>,
    pub original_sha256: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AppIntegrationReceipt {
    schema_version: u32,
    app_path: String,
    app_version: Option<String>,
    original_sha256: String,
    endpoint: String,
    wrapper_sha256: String,
}

struct AppIntegrationPaths {
    app_path: PathBuf,
    bin_dir: PathBuf,
    wrapper_path: PathBuf,
    real_bin_path: PathBuf,
    receipt_path: PathBuf,
}

impl AppIntegrationPaths {
    fn new(app_path: impl AsRef<Path>) -> Self {
        let app_path = app_path.as_ref().to_path_buf();
        let bin_dir = app_path.join("Contents/Resources/bin");
        Self {
            app_path,
            wrapper_path: bin_dir.join("language_server"),
            real_bin_path: bin_dir.join("language_server.real"),
            receipt_path: bin_dir.join(RECEIPT_FILE_NAME),
            bin_dir,
        }
    }
}

pub fn inspect_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    validate_local_endpoint(endpoint)?;
    let paths = AppIntegrationPaths::new(app_path);
    ensure_bundle_directories(&paths)?;
    let app_version = read_app_version(&paths.app_path)?;
    let wrapper_exists = path_exists(&paths.wrapper_path)?;
    let real_exists = path_exists(&paths.real_bin_path)?;
    let receipt_exists = path_exists(&paths.receipt_path)?;

    if !wrapper_exists && !real_exists && !receipt_exists {
        return Ok(disabled_status(paths, app_version));
    }

    if wrapper_exists && !real_exists && !receipt_exists {
        ensure_regular_file(&paths.wrapper_path, "language_server")?;
        let wrapper_bytes = read_file(&paths.wrapper_path)?;
        if is_managed_wrapper_bytes(&wrapper_bytes) {
            return Ok(conflict_status(
                paths,
                app_version,
                "检测到不完整的代理 Wrapper，但原始 language_server 缺失；已阻止继续修改"
                    .to_string(),
            ));
        }
        return Ok(disabled_status(paths, app_version));
    }

    if !wrapper_exists || !real_exists {
        return Ok(conflict_status(
            paths,
            app_version,
            "language_server 与 language_server.real 状态不完整，未执行覆盖操作".to_string(),
        ));
    }

    ensure_regular_file(&paths.wrapper_path, "language_server")?;
    ensure_regular_file(&paths.real_bin_path, "language_server.real")?;
    let wrapper_content = match read_utf8(&paths.wrapper_path) {
        Ok(content) => content,
        Err(_) => {
            return Ok(conflict_status(
                paths,
                app_version,
                "language_server.real 存在，但 language_server 不是有效的文本 Wrapper".to_string(),
            ));
        }
    };

    if !receipt_exists {
        let Some(configured_endpoint) = legacy_endpoint_from_wrapper(&wrapper_content) else {
            return Ok(conflict_status(
                paths,
                app_version,
                "language_server.real 存在，但当前 language_server 不是可识别的 AGY BYOK Wrapper"
                    .to_string(),
            ));
        };
        let real_bytes = read_file(&paths.real_bin_path)?;
        let endpoint_matches = configured_endpoint == endpoint;
        return Ok(AppIntegrationStatus {
            state: if endpoint_matches {
                AppIntegrationState::Managed
            } else {
                AppIntegrationState::Mismatch
            },
            app_path: paths.app_path,
            endpoint_matches,
            configured_endpoint: Some(configured_endpoint),
            app_version,
            original_sha256: Some(sha256(&real_bytes)),
            message: if endpoint_matches {
                format!(
                    "检测到旧版 AGY BYOK Wrapper，App 将使用本地代理 {endpoint}；停用后重新启用可生成新的安全凭据"
                )
            } else {
                format!(
                    "检测到旧版 AGY BYOK Wrapper，当前地址为其他代理；更新或停用后可迁移到安全凭据"
                )
            },
        });
    }

    ensure_regular_file(&paths.receipt_path, "接入凭据")?;
    if !is_managed_wrapper(&wrapper_content) {
        return Ok(conflict_status(
            paths,
            app_version,
            "接入凭据存在，但当前 language_server 不是新版 AGY BYOK Wrapper".to_string(),
        ));
    }

    let receipt = match read_receipt(&paths.receipt_path) {
        Ok(receipt) => receipt,
        Err(error) => {
            return Ok(conflict_status(
                paths,
                app_version,
                format!("无法读取 AGY BYOK 接入凭据：{error}"),
            ));
        }
    };
    let real_bytes = read_file(&paths.real_bin_path)?;
    let actual_original_sha256 = sha256(&real_bytes);
    let wrapper_sha256 = sha256(wrapper_content.as_bytes());
    let configured_endpoint = endpoint_from_wrapper(&wrapper_content);

    let mut conflicts = Vec::new();
    if receipt.schema_version != APP_INTEGRATION_SCHEMA_VERSION {
        conflicts.push(format!("接入凭据版本 {} 不受支持", receipt.schema_version));
    }
    if receipt.app_path != paths.app_path.display().to_string() {
        conflicts.push("接入凭据不属于当前 App".to_string());
    }
    if receipt.original_sha256 != actual_original_sha256 {
        conflicts.push("原始 language_server 已被替换或升级".to_string());
    }
    if receipt.wrapper_sha256 != wrapper_sha256 {
        conflicts.push("AGY BYOK Wrapper 已被外部修改".to_string());
    }
    if receipt.app_version != app_version {
        conflicts.push("Antigravity App 版本已发生变化".to_string());
    }
    if configured_endpoint.as_deref() != Some(receipt.endpoint.as_str()) {
        conflicts.push("Wrapper 中的代理地址与接入凭据不一致".to_string());
    }

    if !conflicts.is_empty() {
        return Ok(conflict_status(
            paths,
            app_version,
            format!("{}；未执行覆盖操作", conflicts.join("；")),
        ));
    }

    let endpoint_matches = receipt.endpoint == endpoint;
    Ok(AppIntegrationStatus {
        state: if endpoint_matches {
            AppIntegrationState::Managed
        } else {
            AppIntegrationState::Mismatch
        },
        app_path: paths.app_path,
        endpoint_matches,
        configured_endpoint: Some(receipt.endpoint.clone()),
        app_version,
        original_sha256: Some(receipt.original_sha256),
        message: if endpoint_matches {
            format!("language_server 已由 AGY BYOK 管理，App 将使用本地代理 {endpoint}")
        } else {
            format!(
                "language_server 已接入其他代理地址 {}，当前代理为 {endpoint}",
                receipt.endpoint
            )
        },
    })
}

pub fn enable_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    validate_local_endpoint(endpoint)?;
    let paths = AppIntegrationPaths::new(app_path);
    ensure_bundle_directories(&paths)?;
    let current = inspect_app_integration(&paths.app_path, endpoint)?;
    if current.state == AppIntegrationState::Conflict {
        return Err(HostIntegrationError::AppIntegrationConflict(
            current.message,
        ));
    }

    let app_version = read_app_version(&paths.app_path)?;
    let (original_sha256, had_existing_wrapper, previous_wrapper, previous_receipt) =
        if current.state == AppIntegrationState::Disabled {
            ensure_regular_file(&paths.wrapper_path, "language_server")?;
            let original_bytes = read_file(&paths.wrapper_path)?;
            if is_managed_wrapper_bytes(&original_bytes) {
                return Err(HostIntegrationError::AppIntegrationConflict(
                    "language_server 看起来已经是 Wrapper，但缺少可验证的原始二进制".to_string(),
                ));
            }
            (sha256(&original_bytes), false, None, None)
        } else {
            ensure_regular_file(&paths.wrapper_path, "language_server")?;
            ensure_regular_file(&paths.real_bin_path, "language_server.real")?;
            let wrapper = read_file(&paths.wrapper_path)?;
            let receipt = if path_exists(&paths.receipt_path)? {
                Some(read_receipt_required(&paths.receipt_path)?)
            } else {
                None
            };
            let original_sha256 = match receipt.as_ref() {
                Some(item) => item.original_sha256.clone(),
                None => sha256(&read_file(&paths.real_bin_path)?),
            };
            (original_sha256, true, Some(wrapper), receipt)
        };

    if !had_existing_wrapper {
        fs::rename(&paths.wrapper_path, &paths.real_bin_path)
            .map_err(|error| io_error(&paths.wrapper_path, error))?;
    }

    let wrapper = wrapper_script(endpoint);
    if let Err(error) = write_atomic(&paths.wrapper_path, wrapper.as_bytes(), 0o755) {
        if !had_existing_wrapper {
            let _ = fs::rename(&paths.real_bin_path, &paths.wrapper_path);
        }
        return Err(error);
    }

    let receipt = AppIntegrationReceipt {
        schema_version: APP_INTEGRATION_SCHEMA_VERSION,
        app_path: paths.app_path.display().to_string(),
        app_version,
        original_sha256,
        endpoint: endpoint.to_string(),
        wrapper_sha256: sha256(wrapper.as_bytes()),
    };
    if let Err(error) = write_receipt(&paths.receipt_path, &receipt) {
        if let Some(previous_wrapper) = previous_wrapper {
            let _ = write_atomic(&paths.wrapper_path, &previous_wrapper, 0o755);
        } else {
            let _ = fs::remove_file(&paths.wrapper_path);
            let _ = fs::rename(&paths.real_bin_path, &paths.wrapper_path);
        }
        if let Some(previous_receipt) = previous_receipt {
            let _ = write_receipt(&paths.receipt_path, &previous_receipt);
        } else {
            let _ = fs::remove_file(&paths.receipt_path);
        }
        return Err(error);
    }

    inspect_app_integration(&paths.app_path, endpoint)
}

pub fn disable_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    validate_local_endpoint(endpoint)?;
    let paths = AppIntegrationPaths::new(app_path);
    ensure_bundle_directories(&paths)?;
    let current = inspect_app_integration(&paths.app_path, endpoint)?;
    match current.state {
        AppIntegrationState::Disabled => return Ok(current),
        AppIntegrationState::Conflict => {
            return Err(HostIntegrationError::AppIntegrationConflict(
                current.message,
            ));
        }
        AppIntegrationState::Managed | AppIntegrationState::Mismatch => {}
    }

    ensure_regular_file(&paths.wrapper_path, "language_server")?;
    ensure_regular_file(&paths.real_bin_path, "language_server.real")?;
    let receipt_exists = path_exists(&paths.receipt_path)?;
    if receipt_exists {
        ensure_regular_file(&paths.receipt_path, "接入凭据")?;
    }

    let backup_path = temporary_path(&paths.wrapper_path);
    fs::rename(&paths.wrapper_path, &backup_path)
        .map_err(|error| io_error(&paths.wrapper_path, error))?;
    if let Err(error) = fs::rename(&paths.real_bin_path, &paths.wrapper_path) {
        let _ = fs::rename(&backup_path, &paths.wrapper_path);
        return Err(io_error(&paths.real_bin_path, error));
    }

    if receipt_exists {
        if let Err(error) = fs::remove_file(&paths.receipt_path) {
            let _ = fs::rename(&paths.wrapper_path, &paths.real_bin_path);
            let _ = fs::rename(&backup_path, &paths.wrapper_path);
            return Err(io_error(&paths.receipt_path, error));
        }
    }
    fs::remove_file(&backup_path).map_err(|error| io_error(&backup_path, error))?;

    inspect_app_integration(&paths.app_path, endpoint)
}

fn ensure_bundle_directories(paths: &AppIntegrationPaths) -> Result<(), HostIntegrationError> {
    ensure_directory(&paths.app_path, "Antigravity.app")?;
    ensure_directory(&paths.bin_dir, "App Resources/bin")?;
    Ok(())
}

fn ensure_directory(path: &Path, label: &str) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{label} cannot be a symbolic link: {}",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{label} is not a directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_regular_file(path: &Path, label: &str) -> Result<(), HostIntegrationError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{label} cannot be a symbolic link: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{label} is not a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, HostIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, error)),
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, HostIntegrationError> {
    fs::read(path).map_err(|error| io_error(path, error))
}

fn read_utf8(path: &Path) -> Result<String, HostIntegrationError> {
    String::from_utf8(read_file(path)?).map_err(|_| {
        HostIntegrationError::InvalidBundle(format!("{} is not valid UTF-8", path.display()))
    })
}

fn read_app_version(app_path: &Path) -> Result<Option<String>, HostIntegrationError> {
    let info_path = app_path.join("Contents/Info.plist");
    if !path_exists(&info_path)? {
        return Ok(None);
    }
    ensure_regular_file(&info_path, "Info.plist")?;
    let value =
        PlistValue::from_file(&info_path).map_err(|source| HostIntegrationError::Plist {
            path: info_path.clone(),
            source,
        })?;
    let dictionary = value.as_dictionary().ok_or_else(|| {
        HostIntegrationError::InvalidBundle("Info.plist root must be a dictionary".to_string())
    })?;
    Ok(dictionary
        .get("CFBundleShortVersionString")
        .and_then(PlistValue::as_string)
        .or_else(|| {
            dictionary
                .get("CFBundleVersion")
                .and_then(PlistValue::as_string)
        })
        .map(ToOwned::to_owned))
}

fn read_receipt(path: &Path) -> Result<AppIntegrationReceipt, HostIntegrationError> {
    let bytes = read_file(path)?;
    serde_json::from_slice(&bytes).map_err(|source| HostIntegrationError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn read_receipt_required(path: &Path) -> Result<AppIntegrationReceipt, HostIntegrationError> {
    read_receipt(path).map_err(|error| {
        HostIntegrationError::AppIntegrationConflict(format!(
            "无法读取接入凭据 {}：{error}",
            path.display()
        ))
    })
}

fn write_receipt(path: &Path, receipt: &AppIntegrationReceipt) -> Result<(), HostIntegrationError> {
    let bytes =
        serde_json::to_vec_pretty(receipt).map_err(|source| HostIntegrationError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    write_atomic(path, &bytes, 0o600)
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), HostIntegrationError> {
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(bytes)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
            .map_err(|error| io_error(&temporary, error))?;
        fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("language_server");
    path.with_file_name(format!(
        ".{file_name}.tmp-{}-{timestamp}",
        std::process::id()
    ))
}

fn validate_local_endpoint(endpoint: &str) -> Result<(), HostIntegrationError> {
    let Some(port) = endpoint.strip_prefix("http://127.0.0.1:") else {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "App 接入只允许使用本地代理地址，收到 {endpoint}"
        )));
    };
    let valid_port = !port.is_empty()
        && port.chars().all(|character| character.is_ascii_digit())
        && port.parse::<u16>().map(|value| value > 0).unwrap_or(false);
    if !valid_port {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "App 接入端口无效：{endpoint}"
        )));
    }
    Ok(())
}

fn wrapper_script(endpoint: &str) -> String {
    format!(
        r#"#!/bin/bash
{WRAPPER_MARKER}
{ENDPOINT_MARKER}{endpoint}
set -e
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
    )
}

fn is_managed_wrapper(content: &str) -> bool {
    content.lines().any(|line| line.trim() == WRAPPER_MARKER)
}

fn is_managed_wrapper_bytes(bytes: &[u8]) -> bool {
    String::from_utf8(bytes.to_vec())
        .map(|content| {
            is_managed_wrapper(&content) || legacy_endpoint_from_wrapper(&content).is_some()
        })
        .unwrap_or(false)
}

fn endpoint_from_wrapper(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(ENDPOINT_MARKER).map(str::to_string))
}

fn legacy_endpoint_from_wrapper(content: &str) -> Option<String> {
    let expected_structure = content.contains("#!/bin/bash")
        && content.contains("DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"")
        && content.contains("if [ \"$arg\" = \"")
        && content.contains(TARGET_OFFICIAL_ENDPOINT)
        && content.contains("exec \"$DIR/language_server.real\"");
    if !expected_structure {
        return None;
    }

    content.lines().find_map(|line| {
        let value = line.trim().strip_prefix("ARGS+=(\"")?.strip_suffix("\")")?;
        if value == "$arg" || validate_local_endpoint(value).is_err() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

fn disabled_status(
    paths: AppIntegrationPaths,
    app_version: Option<String>,
) -> AppIntegrationStatus {
    AppIntegrationStatus {
        state: AppIntegrationState::Disabled,
        app_path: paths.app_path,
        endpoint_matches: false,
        configured_endpoint: None,
        app_version,
        original_sha256: None,
        message: "原始 language_server 已就位，App 使用官方服务".to_string(),
    }
}

fn conflict_status(
    paths: AppIntegrationPaths,
    app_version: Option<String>,
    message: String,
) -> AppIntegrationStatus {
    AppIntegrationStatus {
        state: AppIntegrationState::Conflict,
        app_path: paths.app_path,
        endpoint_matches: false,
        configured_endpoint: None,
        app_version,
        original_sha256: None,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Value};

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

        let mismatch =
            inspect_app_integration(&fixture.app_path, "http://127.0.0.1:56067").unwrap();
        assert_eq!(mismatch.state, AppIntegrationState::Mismatch);
        assert_eq!(mismatch.configured_endpoint.as_deref(), Some(endpoint));

        let disabled = disable_app_integration(&fixture.app_path, endpoint).unwrap();
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
}
