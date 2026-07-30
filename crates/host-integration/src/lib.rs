mod discovery;
mod error;
mod ide_settings;
mod profile;
mod signing;
mod transaction;

pub use discovery::{discover, HostInstallation};
pub use error::HostIntegrationError;
pub use ide_settings::{
    disable_ide_settings, enable_ide_settings, inspect_ide_settings, IdeSettingsState,
    IdeSettingsStatus, IDE_CLOUD_CODE_SETTING, IDE_SETTINGS_BACKUP_FILE, IDE_SETTINGS_RECEIPT_FILE,
};

pub use profile::{HostLayout, InstallationState, PatchProfile};
pub use signing::{CodeSignatureVerifier, MacOsCodeSignatureVerifier};

pub use transaction::{
    dry_run, restore, BundleSnapshotStrategy, PatchReceipt, PatchTransactionState,
};

use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
