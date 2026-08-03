use super::{ownership, patch, AppIntegrationState, AppIntegrationStatus};
use crate::error::{io_error, HostIntegrationError};
use crate::sha256;
use plist::Value as PlistValue;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct AppIntegrationPaths {
    pub(super) app_path: PathBuf,
    pub(super) bin_dir: PathBuf,
    pub(super) wrapper_path: PathBuf,
    pub(super) real_bin_path: PathBuf,
    pub(super) receipt_path: PathBuf,
}

impl AppIntegrationPaths {
    pub(super) fn new(app_path: impl AsRef<Path>) -> Self {
        let app_path = app_path.as_ref().to_path_buf();
        let bin_dir = app_path.join("Contents/Resources/bin");
        Self {
            app_path,
            wrapper_path: bin_dir.join("language_server"),
            real_bin_path: bin_dir.join("language_server.real"),
            receipt_path: bin_dir.join(super::RECEIPT_FILE_NAME),
            bin_dir,
        }
    }
}

pub(super) fn inspect_app_integration(
    app_path: impl AsRef<Path>,
    endpoint: &str,
) -> Result<AppIntegrationStatus, HostIntegrationError> {
    patch::validate_local_endpoint(endpoint)?;
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
        if patch::is_managed_wrapper_bytes(&wrapper_bytes) {
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
        let Some(configured_endpoint) = patch::legacy_endpoint_from_wrapper(&wrapper_content)
        else {
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
                "检测到旧版 AGY BYOK Wrapper，当前地址为其他代理；更新或停用后可迁移到安全凭据"
                    .to_string()
            },
        });
    }

    ensure_regular_file(&paths.receipt_path, "接入凭据")?;
    if !patch::is_managed_wrapper(&wrapper_content) {
        return Ok(conflict_status(
            paths,
            app_version,
            "接入凭据存在，但当前 language_server 不是新版 AGY BYOK Wrapper".to_string(),
        ));
    }

    let receipt = match ownership::read_receipt(&paths.receipt_path) {
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
    let configured_endpoint = patch::endpoint_from_wrapper(&wrapper_content);

    let mut conflicts = Vec::new();
    if receipt.schema_version != super::APP_INTEGRATION_SCHEMA_VERSION {
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

pub(super) fn ensure_bundle_directories(
    paths: &AppIntegrationPaths,
) -> Result<(), HostIntegrationError> {
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

pub(super) fn ensure_regular_file(path: &Path, label: &str) -> Result<(), HostIntegrationError> {
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

pub(super) fn path_exists(path: &Path) -> Result<bool, HostIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, error)),
    }
}

pub(super) fn read_file(path: &Path) -> Result<Vec<u8>, HostIntegrationError> {
    fs::read(path).map_err(|error| io_error(path, error))
}

fn read_utf8(path: &Path) -> Result<String, HostIntegrationError> {
    String::from_utf8(read_file(path)?).map_err(|_| {
        HostIntegrationError::InvalidBundle(format!("{} is not valid UTF-8", path.display()))
    })
}

pub(super) fn read_app_version(app_path: &Path) -> Result<Option<String>, HostIntegrationError> {
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
