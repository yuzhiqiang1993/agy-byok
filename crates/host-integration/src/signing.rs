use crate::error::HostIntegrationError;
use std::path::Path;
use std::process::{Command, Output};

const GOOGLE_AUTHORITY_FRAGMENT: &str = "Google LLC";

pub trait CodeSignatureVerifier {
    fn verify_vendor(
        &self,
        app_path: &Path,
        expected_bundle_id: &str,
    ) -> Result<(), HostIntegrationError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsCodeSignatureVerifier;

impl CodeSignatureVerifier for MacOsCodeSignatureVerifier {
    fn verify_vendor(
        &self,
        app_path: &Path,
        expected_bundle_id: &str,
    ) -> Result<(), HostIntegrationError> {
        let verification = run_codesign(
            ["--verify", "--deep", "--strict", "--all-architectures"],
            app_path,
        )?;
        ensure_success("verify", verification)?;

        let details = run_codesign(["-dv", "--verbose=4"], app_path)?;
        ensure_success("display signature", details.clone())?;
        let output = command_output(&details);
        let expected_identifier = format!("Identifier={expected_bundle_id}");
        let has_expected_identifier = output.lines().any(|line| line == expected_identifier);
        let has_google_authority = output
            .lines()
            .filter_map(|line| line.strip_prefix("Authority="))
            .any(|authority| authority.contains(GOOGLE_AUTHORITY_FRAGMENT));
        let is_ad_hoc = output.lines().any(|line| line == "Signature=adhoc");

        if !has_expected_identifier || !has_google_authority || is_ad_hoc {
            return Err(HostIntegrationError::CommandFailed(format!(
                "application signature is not the expected Google vendor identity: {output}"
            )));
        }
        Ok(())
    }
}

fn run_codesign<const N: usize>(
    arguments: [&str; N],
    app_path: &Path,
) -> Result<Output, HostIntegrationError> {
    if !cfg!(target_os = "macos") {
        return Err(HostIntegrationError::CommandFailed(
            "codesign verification is only supported on macOS".to_string(),
        ));
    }
    Command::new("/usr/bin/codesign")
        .args(arguments)
        .arg(app_path)
        .output()
        .map_err(|error| {
            HostIntegrationError::CommandFailed(format!(
                "failed to start read-only codesign verification: {error}"
            ))
        })
}

fn ensure_success(operation: &str, output: Output) -> Result<(), HostIntegrationError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(HostIntegrationError::CommandFailed(format!(
            "codesign {operation} failed with status {}: {}",
            output.status,
            command_output(&output)
        )))
    }
}

fn command_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}").trim().to_string()
}
