use crate::native_ui::NativeLocale;
use agy_byok::domain::ConfigError;
use agy_byok::storage::ConfigStoreError;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum StartupError {
    ConfigPath(String),
    MissingConfigParent(PathBuf),
    Config {
        path: PathBuf,
        source: ConfigStoreError,
    },
}

impl StartupError {
    pub fn user_message(&self, locale: NativeLocale) -> String {
        match (locale, self) {
            (NativeLocale::ZhCn, Self::Config { path, source }) => format!(
                "配置文件无法加载，AGY BYOK 未修改该文件。\n\n文件：{}\n原因：{}\n\n请先备份并修正配置文件，或手动移走该文件后重新启动。",
                path.display(),
                config_failure_reason(NativeLocale::ZhCn, source),
            ),
            (NativeLocale::EnUs, Self::Config { path, source }) => format!(
                "The configuration file could not be loaded and was not modified.\n\nFile: {}\nReason: {}\n\nBack up and fix the file, or move it elsewhere before restarting AGY BYOK.",
                path.display(),
                config_failure_reason(NativeLocale::EnUs, source),
            ),
            (NativeLocale::ZhCn, Self::ConfigPath(_)) => {
                "AGY BYOK 无法定位配置文件。请确认当前用户目录可用后重新启动。".to_string()
            }
            (NativeLocale::EnUs, Self::ConfigPath(_)) =>
                "AGY BYOK could not locate its configuration file. Verify that the current user directory is available, then restart the app.".to_string(),
            (NativeLocale::ZhCn, Self::MissingConfigParent(path)) => format!(
                "配置文件路径缺少父目录。\n\n路径：{}",
                path.display()
            ),
            (NativeLocale::EnUs, Self::MissingConfigParent(path)) => format!(
                "The configuration path has no parent directory.\n\nPath: {}",
                path.display()
            ),
        }
    }
}

fn config_failure_reason(locale: NativeLocale, error: &ConfigStoreError) -> String {
    match (locale, error) {
        (NativeLocale::ZhCn, ConfigStoreError::InvalidFileType { .. }) => {
            "配置路径不是常规文件，已拒绝读取符号链接或目录".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::InvalidFileType { .. }) => {
            "The configuration path is not a regular file; symbolic links and directories are not accepted".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::SecurePermissions { .. }) => {
            "无法将配置文件权限收紧为仅当前用户可读写".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::SecurePermissions { .. }) => {
            "The configuration file permissions could not be restricted to the current user".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::Read { .. }) => {
            "无法读取文件，请检查文件权限和磁盘状态".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::Read { .. }) => {
            "The file could not be read; check its permissions and the disk state".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::DeleteIncompatible { .. }) => {
            "检测到配置与当前版本不兼容，但无法删除该文件，请检查目录权限".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::DeleteIncompatible { .. }) => {
            "The configuration is incompatible with this version but could not be deleted; check the directory permissions".to_string()
        }
        (locale, ConfigStoreError::Parse { source, .. }) => parse_failure_reason(locale, source),
        (locale, ConfigStoreError::Invalid(source)) => validation_failure_reason(locale, source),
        (NativeLocale::ZhCn, ConfigStoreError::Serialize(_)) => {
            "配置内容无法序列化".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::Serialize(_)) => {
            "The configuration could not be serialized".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::CreateDirectory { .. }) => {
            "无法创建配置目录，请检查目录权限".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::CreateDirectory { .. }) => {
            "The configuration directory could not be created; check its permissions".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::Write { .. }) => {
            "无法写入临时配置文件，请检查目录权限和磁盘空间".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::Write { .. }) => {
            "The temporary configuration file could not be written; check directory permissions and disk space".to_string()
        }
        (NativeLocale::ZhCn, ConfigStoreError::Replace { .. }) => {
            "无法原子替换配置文件，请检查文件和目录权限".to_string()
        }
        (NativeLocale::EnUs, ConfigStoreError::Replace { .. }) => {
            "The configuration file could not be replaced atomically; check file and directory permissions".to_string()
        }
    }
}

fn parse_failure_reason(locale: NativeLocale, error: &serde_json::Error) -> String {
    match (locale, error.classify()) {
        (NativeLocale::ZhCn, serde_json::error::Category::Data) => format!(
            "配置结构与当前版本不匹配，位于第 {} 行、第 {} 列",
            error.line(),
            error.column()
        ),
        (NativeLocale::EnUs, serde_json::error::Category::Data) => format!(
            "The configuration structure does not match this version at line {}, column {}",
            error.line(),
            error.column()
        ),
        (
            NativeLocale::ZhCn,
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof,
        ) => format!(
            "JSON 语法错误，位于第 {} 行、第 {} 列",
            error.line(),
            error.column()
        ),
        (
            NativeLocale::EnUs,
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof,
        ) => format!(
            "JSON syntax error at line {}, column {}",
            error.line(),
            error.column()
        ),
        (NativeLocale::ZhCn, serde_json::error::Category::Io) => {
            "配置内容无法解析，请检查文件是否可读".to_string()
        }
        (NativeLocale::EnUs, serde_json::error::Category::Io) => {
            "The configuration could not be parsed; check that the file is readable".to_string()
        }
    }
}

fn validation_failure_reason(locale: NativeLocale, error: &ConfigError) -> String {
    match (locale, error) {
        (NativeLocale::ZhCn, ConfigError::InvalidValue(_)) => {
            "配置值未通过校验，请检查代理端口、模型标识、服务端点和压缩设置".to_string()
        }
        (NativeLocale::EnUs, ConfigError::InvalidValue(_)) => {
            "A value failed validation; check the proxy port, model identifiers, service endpoints, and compression settings".to_string()
        }
        (NativeLocale::ZhCn, ConfigError::DuplicateId { kind, id }) => {
            format!("{kind} 标识重复：{id}")
        }
        (NativeLocale::EnUs, ConfigError::DuplicateId { kind, id }) => {
            format!("Duplicate {kind} identifier: {id}")
        }
        (
            NativeLocale::ZhCn,
            ConfigError::MissingReference {
                owner_kind,
                owner_id,
                target_kind,
                target_id,
            },
        ) => format!("{owner_kind} {owner_id} 引用了不存在的 {target_kind}：{target_id}"),
        (
            NativeLocale::EnUs,
            ConfigError::MissingReference {
                owner_kind,
                owner_id,
                target_kind,
                target_id,
            },
        ) => format!("{owner_kind} {owner_id} references missing {target_kind}: {target_id}"),
        (
            NativeLocale::ZhCn,
            ConfigError::IdentifierConflict {
                model_id,
                existing_model_id,
                identifier,
            },
        ) => format!(
            "VirtualModel {model_id} 与 {existing_model_id} 使用了相同标识：{identifier}"
        ),
        (
            NativeLocale::EnUs,
            ConfigError::IdentifierConflict {
                model_id,
                existing_model_id,
                identifier,
            },
        ) => format!(
            "VirtualModel {model_id} and {existing_model_id} use the same identifier: {identifier}"
        ),
        (
            NativeLocale::ZhCn,
            ConfigError::UnsupportedReasoning { model_id, level },
        ) => format!("VirtualModel {model_id} 使用了不支持的推理等级：{level}"),
        (
            NativeLocale::EnUs,
            ConfigError::UnsupportedReasoning { model_id, level },
        ) => format!("VirtualModel {model_id} uses unsupported reasoning level: {level}"),
    }
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigPath(message) => write!(formatter, "无法定位配置文件：{message}"),
            Self::MissingConfigParent(path) => {
                write!(formatter, "配置路径缺少父目录：{}", path.display())
            }
            Self::Config { path, source } => {
                write!(
                    formatter,
                    "配置文件 {} 初始化失败：{source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_is_localized_without_internal_text() {
        let path = PathBuf::from("/tmp/agy-byok-invalid-config.json");
        let error = StartupError::Config {
            path: path.clone(),
            source: ConfigStoreError::Invalid(ConfigError::InvalidValue(
                "自定义压缩百分比必须在 1 到 100 之间".to_string(),
            )),
        };

        let message = error.user_message(NativeLocale::EnUs);

        assert!(message.contains(path.to_str().unwrap()));
        assert!(message.contains("failed validation"));
        assert!(!message.contains("百分比"));
    }

    #[test]
    fn parse_error_reports_localized_location() {
        let path = PathBuf::from("/tmp/agy-byok-invalid-config.json");
        let source =
            serde_json::from_str::<serde_json::Value>("{\n  invalid").expect_err("invalid JSON");
        let error = StartupError::Config {
            path,
            source: ConfigStoreError::Parse {
                path: PathBuf::from("/tmp/agy-byok-invalid-config.json"),
                source,
            },
        };

        let message = error.user_message(NativeLocale::ZhCn);

        assert!(message.contains("JSON 语法错误"));
        assert!(message.contains("第 2 行"));
    }

    #[test]
    fn data_error_is_reported_as_a_schema_mismatch() {
        let path = PathBuf::from("/tmp/agy-byok-incompatible-config.json");
        let source = serde_json::from_str::<bool>(r#""safe""#)
            .expect_err("a string must not deserialize as a boolean");
        let error = StartupError::Config {
            path,
            source: ConfigStoreError::Parse {
                path: PathBuf::from("/tmp/agy-byok-incompatible-config.json"),
                source,
            },
        };

        let message = error.user_message(NativeLocale::ZhCn);

        assert!(message.contains("配置结构与当前版本不匹配"));
        assert!(!message.contains("JSON 语法错误"));
    }
}
