use crate::error::{io_error, HostIntegrationError};
use crate::profile::{safe_join, HostLayout};
use plist::Value as PlistValue;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInstallation {
    pub app_path: PathBuf,
    pub bundle_id: String,
    pub app_version: String,
    pub extension_version: String,
    pub extension_sha256: String,
}

#[derive(Debug, Deserialize)]
struct ExtensionPackage {
    version: String,
}

pub fn discover(
    app_path: impl AsRef<Path>,
    layout: &HostLayout,
) -> Result<HostInstallation, HostIntegrationError> {
    layout.validate()?;
    let app_path = app_path.as_ref();
    if !app_path.is_dir() {
        return Err(HostIntegrationError::InvalidBundle(format!(
            "{} is not an application directory",
            app_path.display()
        )));
    }

    let info_path = safe_join(app_path, &layout.info_plist)?;
    let info = PlistValue::from_file(&info_path).map_err(|source| HostIntegrationError::Plist {
        path: info_path.clone(),
        source,
    })?;
    let dictionary = info.as_dictionary().ok_or_else(|| {
        HostIntegrationError::InvalidBundle("Info.plist root must be a dictionary".to_string())
    })?;
    let bundle_id = plist_string(dictionary, "CFBundleIdentifier")?;
    let app_version = plist_string(dictionary, "CFBundleShortVersionString")?;

    let package_path = safe_join(app_path, &layout.extension_package)?;
    let package_bytes =
        fs::read(&package_path).map_err(|source| io_error(&package_path, source))?;
    let package: ExtensionPackage =
        serde_json::from_slice(&package_bytes).map_err(|source| HostIntegrationError::Json {
            path: package_path,
            source,
        })?;

    let extension_path = safe_join(app_path, &layout.extension_entry)?;
    let extension_bytes =
        fs::read(&extension_path).map_err(|source| io_error(&extension_path, source))?;

    Ok(HostInstallation {
        app_path: app_path.to_path_buf(),
        bundle_id,
        app_version,
        extension_version: package.version,
        extension_sha256: crate::sha256(&extension_bytes),
    })
}

fn plist_string(dictionary: &plist::Dictionary, key: &str) -> Result<String, HostIntegrationError> {
    dictionary
        .get(key)
        .and_then(PlistValue::as_string)
        .map(ToOwned::to_owned)
        .ok_or_else(|| HostIntegrationError::InvalidBundle(format!("Info.plist is missing {key}")))
}
