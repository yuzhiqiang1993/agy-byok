mod discovery;
mod error;
mod managed_copy;
mod profile;
mod signing;
mod transaction;

pub use discovery::{discover, HostInstallation};
pub use error::HostIntegrationError;
pub use managed_copy::{
    create_managed_copy, inspect_managed_copy, remove_managed_copy, ManagedCopyReceipt,
    ManagedCopyResult, ManagedCopyState, MANAGED_APP_NAME, MANAGED_RECEIPT_FILE,
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
