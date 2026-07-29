use crate::discovery::HostInstallation;
use crate::error::HostIntegrationError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const IDE_EXTENSION_RELATIVE_PATH: &str =
    "Contents/Resources/app/extensions/antigravity/dist/extension.js";
pub const IDE_EXTENSION_PACKAGE_RELATIVE_PATH: &str =
    "Contents/Resources/app/extensions/antigravity/package.json";
pub const IDE_INFO_PLIST_RELATIVE_PATH: &str = "Contents/Info.plist";
pub const IDE_SIGNATURE_RELATIVE_PATH: &str = "Contents/_CodeSignature";
pub const IDE_CODE_RESOURCES_RELATIVE_PATH: &str = "Contents/CodeResources";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostLayout {
    pub info_plist: PathBuf,
    pub extension_package: PathBuf,
    pub extension_entry: PathBuf,
    pub signature_directory: PathBuf,
    pub code_resources: PathBuf,
}

impl HostLayout {
    pub fn antigravity_ide() -> Self {
        Self {
            info_plist: IDE_INFO_PLIST_RELATIVE_PATH.into(),
            extension_package: IDE_EXTENSION_PACKAGE_RELATIVE_PATH.into(),
            extension_entry: IDE_EXTENSION_RELATIVE_PATH.into(),
            signature_directory: IDE_SIGNATURE_RELATIVE_PATH.into(),
            code_resources: IDE_CODE_RESOURCES_RELATIVE_PATH.into(),
        }
    }

    pub fn validate(&self) -> Result<(), HostIntegrationError> {
        for path in [
            &self.info_plist,
            &self.extension_package,
            &self.extension_entry,
            &self.signature_directory,
            &self.code_resources,
        ] {
            validate_relative_path(path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchProfile {
    pub id: String,
    pub bundle_id: String,
    pub app_version: String,
    pub extension_version: String,
    pub original_sha256: String,
    pub patched_sha256: String,
    pub endpoint: String,
    pub anchor: String,
    pub replacement: String,
    pub layout: HostLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationState {
    VendorOriginal,
    PatchedByProfile,
    Modified,
}

impl PatchProfile {
    pub fn antigravity_ide_2_1_1() -> Self {
        Self {
            id: "antigravity-ide-macos-arm64-2.1.1-extension-0.2.0".to_string(),
            bundle_id: "com.google.antigravity-ide".to_string(),
            app_version: "2.1.1".to_string(),
            extension_version: "0.2.0".to_string(),
            original_sha256: "a43a4aa5e6189fd185e2deb426ba3feb04433f22a8a4a2182ffb237d0b7a0c3d"
                .to_string(),
            patched_sha256: "709ff74753eaac307fbd78ce39de7811395eabc504fe5fb39a65e226b4871c96"
                .to_string(),
            endpoint: "http://127.0.0.1:50999".to_string(),
            anchor: "const x=await o.getCloudCodeUrl();_.push(\"--cloud_code_endpoint\",x)"
                .to_string(),
            replacement: "const x=\"http://127.0.0.1:50999\";_.push(\"--cloud_code_endpoint\",x)"
                .to_string(),
            layout: HostLayout::antigravity_ide(),
        }
    }

    pub fn classify(
        &self,
        installation: &HostInstallation,
    ) -> Result<InstallationState, HostIntegrationError> {
        self.validate_metadata(installation)?;
        if installation.extension_sha256 == self.original_sha256 {
            Ok(InstallationState::VendorOriginal)
        } else if installation.extension_sha256 == self.patched_sha256 {
            Ok(InstallationState::PatchedByProfile)
        } else {
            Ok(InstallationState::Modified)
        }
    }

    pub fn validate_for_apply(
        &self,
        installation: &HostInstallation,
    ) -> Result<(), HostIntegrationError> {
        match self.classify(installation)? {
            InstallationState::VendorOriginal => Ok(()),
            InstallationState::PatchedByProfile => Err(HostIntegrationError::ProfileMismatch(
                "application already contains this profile patch".to_string(),
            )),
            InstallationState::Modified => Err(HostIntegrationError::ProfileMismatch(format!(
                "extension hash {} is not the vendor baseline {}",
                installation.extension_sha256, self.original_sha256
            ))),
        }
    }

    pub fn create_candidate(&self, source: &str) -> Result<String, HostIntegrationError> {
        let anchor_count = source.matches(&self.anchor).count();
        if anchor_count != 1 {
            return Err(HostIntegrationError::AnchorCount {
                count: anchor_count,
            });
        }
        let candidate = source.replacen(&self.anchor, &self.replacement, 1);
        let actual_hash = crate::sha256(candidate.as_bytes());
        if actual_hash != self.patched_sha256 {
            return Err(HostIntegrationError::HashMismatch {
                expected: self.patched_sha256.clone(),
                actual: actual_hash,
            });
        }
        Ok(candidate)
    }

    fn validate_metadata(
        &self,
        installation: &HostInstallation,
    ) -> Result<(), HostIntegrationError> {
        self.layout.validate()?;
        let mismatches = [
            (
                "bundle ID",
                installation.bundle_id.as_str(),
                self.bundle_id.as_str(),
            ),
            (
                "application version",
                installation.app_version.as_str(),
                self.app_version.as_str(),
            ),
            (
                "extension version",
                installation.extension_version.as_str(),
                self.extension_version.as_str(),
            ),
        ]
        .into_iter()
        .filter(|(_, actual, expected)| actual != expected)
        .map(|(label, actual, expected)| format!("{label}: expected {expected}, found {actual}"))
        .collect::<Vec<_>>();

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(HostIntegrationError::ProfileMismatch(mismatches.join("; ")))
        }
    }
}

pub(crate) fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf, HostIntegrationError> {
    validate_relative_path(relative)?;
    Ok(root.join(relative))
}

fn validate_relative_path(path: &Path) -> Result<(), HostIntegrationError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(HostIntegrationError::UnsafeRelativePath(path.to_path_buf()));
    }
    Ok(())
}
