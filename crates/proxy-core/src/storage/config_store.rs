use crate::domain::{AppConfig, ConfigError};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub enum ConfigStoreError {
    InvalidFileType {
        path: PathBuf,
    },
    SecurePermissions {
        path: PathBuf,
        source: io::Error,
    },
    Read {
        path: PathBuf,
        source: io::Error,
    },
    DeleteIncompatible {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Invalid(ConfigError),
    Serialize(serde_json::Error),
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    Replace {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ConfigStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileType { path } => write!(
                formatter,
                "Config path must be a regular file: {}",
                path.display()
            ),
            Self::SecurePermissions { path, source } => write!(
                formatter,
                "Failed to secure config file permissions {}: {source}",
                path.display()
            ),
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "Failed to read config {}: {source}",
                    path.display()
                )
            }
            Self::DeleteIncompatible { path, source } => write!(
                formatter,
                "Failed to delete incompatible config {}: {source}",
                path.display()
            ),
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "Failed to parse config {}: {source}",
                    path.display()
                )
            }
            Self::Invalid(source) => write!(formatter, "Invalid config: {source}"),
            Self::Serialize(source) => write!(formatter, "Failed to serialize config: {source}"),
            Self::CreateDirectory { path, source } => write!(
                formatter,
                "Failed to create config directory {}: {source}",
                path.display()
            ),
            Self::Write { path, source } => write!(
                formatter,
                "Failed to write temporary config {}: {source}",
                path.display()
            ),
            Self::Replace { path, source } => write!(
                formatter,
                "Failed to replace config file {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::DeleteIncompatible { source, .. }
            | Self::SecurePermissions { source, .. }
            | Self::CreateDirectory { source, .. }
            | Self::Write { source, .. }
            | Self::Replace { source, .. } => Some(source),
            Self::Parse { source, .. } | Self::Serialize(source) => Some(source),
            Self::Invalid(source) => Some(source),
            Self::InvalidFileType { .. } => None,
        }
    }
}

impl From<ConfigError> for ConfigStoreError {
    fn from(source: ConfigError) -> Self {
        Self::Invalid(source)
    }
}

#[derive(Clone)]
pub struct ConfigStore {
    config: Arc<RwLock<AppConfig>>,
    file_path: Option<PathBuf>,
    raw_official_catalog: Arc<RwLock<Option<String>>>,
}

impl ConfigStore {
    pub fn in_memory(initial_config: AppConfig) -> Self {
        initial_config
            .validate()
            .expect("in-memory config must be valid");
        Self {
            config: Arc::new(RwLock::new(initial_config)),
            file_path: None,
            raw_official_catalog: Arc::new(RwLock::new(None)),
        }
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, ConfigStoreError> {
        let path_buf = path.as_ref().to_path_buf();
        let (config, loaded_from_file) = match fs::symlink_metadata(&path_buf) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                secure_config_permissions(&path_buf, &metadata)?;
                let content =
                    fs::read_to_string(&path_buf).map_err(|source| ConfigStoreError::Read {
                        path: path_buf.clone(),
                        source,
                    })?;
                match serde_json::from_str::<AppConfig>(&content) {
                    Ok(config) => (config, true),
                    Err(source) if source.classify() == serde_json::error::Category::Data => {
                        (delete_incompatible_config(&path_buf, &source)?, false)
                    }
                    Err(source) => {
                        return Err(ConfigStoreError::Parse {
                            path: path_buf.clone(),
                            source,
                        });
                    }
                }
            }
            Ok(_) => {
                return Err(ConfigStoreError::InvalidFileType {
                    path: path_buf.clone(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (AppConfig::default(), false),
            Err(source) => {
                return Err(ConfigStoreError::Read {
                    path: path_buf.clone(),
                    source,
                });
            }
        };

        let config = match config.validate() {
            Ok(()) => config,
            Err(source) if loaded_from_file => delete_incompatible_config(&path_buf, &source)?,
            Err(source) => return Err(ConfigStoreError::Invalid(source)),
        };

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            file_path: Some(path_buf),
            raw_official_catalog: Arc::new(RwLock::new(None)),
        })
    }

    pub fn set_raw_official_catalog(&self, catalog: String) {
        if let Ok(mut lock) = self.raw_official_catalog.write() {
            *lock = Some(catalog);
        }
    }

    pub fn get_raw_official_catalog(&self) -> Option<String> {
        self.raw_official_catalog.read().ok()?.clone()
    }
    pub fn get_config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update_config(&self, new_config: AppConfig) -> Result<(), ConfigStoreError> {
        new_config.validate()?;
        let mut guard = self.config.write().unwrap();
        self.persist_config(&new_config)?;
        *guard = new_config;
        Ok(())
    }

    pub fn update_config_with<F>(&self, update: F) -> Result<AppConfig, ConfigStoreError>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut guard = self.config.write().unwrap();
        let mut new_config = guard.clone();
        update(&mut new_config);
        new_config.validate()?;
        self.persist_config(&new_config)?;
        *guard = new_config.clone();
        Ok(new_config)
    }

    fn persist_config(&self, config: &AppConfig) -> Result<(), ConfigStoreError> {
        let Some(path) = &self.file_path else {
            return Ok(());
        };
        let json_content =
            serde_json::to_string_pretty(config).map_err(ConfigStoreError::Serialize)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigStoreError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let temporary_path = temporary_config_path(path);

        if let Err(source) = write_private_file(&temporary_path, json_content.as_bytes()) {
            let _ = fs::remove_file(&temporary_path);
            return Err(ConfigStoreError::Write {
                path: temporary_path.clone(),
                source,
            });
        }
        if let Err(source) = replace_file(&temporary_path, path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(ConfigStoreError::Replace {
                path: path.to_path_buf(),
                source,
            });
        }
        if let Err(error) = sync_parent_directory(path) {
            // 文件替换已经提交，目录同步失败不能再向调用方伪装成“未保存”。
            tracing::warn!(path = %path.display(), %error, "配置文件已替换，但父目录同步失败");
        }
        Ok(())
    }
}

fn delete_incompatible_config(
    path: &Path,
    reason: &dyn fmt::Display,
) -> Result<AppConfig, ConfigStoreError> {
    fs::remove_file(path).map_err(|source| ConfigStoreError::DeleteIncompatible {
        path: path.to_path_buf(),
        source,
    })?;
    tracing::warn!(path = %path.display(), %reason, "已删除与当前版本不兼容的配置文件");
    if let Err(error) = sync_parent_directory(path) {
        tracing::warn!(path = %path.display(), %error, "不兼容配置已删除，但父目录同步失败");
    }
    Ok(AppConfig::default())
}

fn write_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(unix)]
fn secure_config_permissions(path: &Path, metadata: &fs::Metadata) -> Result<(), ConfigStoreError> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o777 == 0o600 {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        ConfigStoreError::SecurePermissions {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn secure_config_permissions(
    _path: &Path,
    _metadata: &fs::Metadata,
) -> Result<(), ConfigStoreError> {
    Ok(())
}

fn temporary_config_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.v1.json");
    path.with_file_name(format!(
        ".{file_name}.{}.next",
        uuid::Uuid::new_v4().simple()
    ))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // Windows 的 rename 不能覆盖既有文件，使用原子替换保证连续保存配置可用。
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
