use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum HostIntegrationError {
    #[error("invalid application bundle: {0}")]
    InvalidBundle(String),
    #[error("host profile mismatch: {0}")]
    ProfileMismatch(String),
    #[error("unsafe relative path in host profile: {0}")]
    UnsafeRelativePath(PathBuf),
    #[error("patch anchor count must be exactly one, found {count}")]
    AnchorCount { count: usize },
    #[error("host file changed outside this transaction: expected {expected}, found {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("receipt does not belong to this application bundle")]
    ReceiptMismatch,
    #[error("IDE settings integration conflict: {0}")]
    SettingsConflict(String),
    #[error("App 接入冲突：{0}")]
    AppIntegrationConflict(String),
    #[error("host command failed: {0}")]
    CommandFailed(String),
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse plist {path}: {source}")]
    Plist {
        path: PathBuf,
        #[source]
        source: plist::Error,
    },
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
