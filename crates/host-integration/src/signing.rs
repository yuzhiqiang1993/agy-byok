use crate::error::HostIntegrationError;
use std::path::Path;
use std::process::Command;

pub trait CodeSigner {
    fn sign(&self, app_path: &Path) -> Result<(), HostIntegrationError>;
    fn verify(&self, app_path: &Path) -> Result<(), HostIntegrationError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsAdHocCodeSigner;

impl CodeSigner for MacOsAdHocCodeSigner {
    fn sign(&self, app_path: &Path) -> Result<(), HostIntegrationError> {
        // Re-sign only the outer bundle. `--deep` signing can mutate nested vendor
        // binaries and makes a file-level rollback incomplete.
        run_codesign(["--force", "--sign", "-"], app_path)
    }

    fn verify(&self, app_path: &Path) -> Result<(), HostIntegrationError> {
        run_codesign(["--verify", "--deep", "--strict"], app_path)
    }
}

fn run_codesign<const N: usize>(
    arguments: [&str; N],
    app_path: &Path,
) -> Result<(), HostIntegrationError> {
    if !cfg!(target_os = "macos") {
        return Err(HostIntegrationError::CommandFailed(
            "codesign is only supported on macOS".to_string(),
        ));
    }
    let status = Command::new("codesign")
        .args(arguments)
        .arg(app_path)
        .status()
        .map_err(|error| {
            HostIntegrationError::CommandFailed(format!("failed to start codesign: {error}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(HostIntegrationError::CommandFailed(format!(
            "codesign exited with status {status}"
        )))
    }
}
