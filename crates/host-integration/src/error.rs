use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum HostIntegrationError {
    #[error("invalid host integration state: {0}")]
    InvalidIntegration(String),
    #[error("IDE settings integration conflict: {0}")]
    SettingsConflict(String),
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(target_os = "macos")]
    #[error("macOS command failed: {0}")]
    Command(String),
    #[error("{operation}; recovery failed: {recovery}")]
    RecoveryFailed { operation: String, recovery: String },
    #[error("failed to parse JSON {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

pub(crate) fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> HostIntegrationError {
    HostIntegrationError::Io {
        path: path.into(),
        source,
    }
}
