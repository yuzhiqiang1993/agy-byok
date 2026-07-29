mod discovery;
mod error;
mod profile;
mod signing;
mod transaction;

pub use discovery::{discover, HostInstallation};
pub use error::HostIntegrationError;
pub use profile::{HostLayout, InstallationState, PatchProfile};
pub use signing::{CodeSigner, MacOsAdHocCodeSigner};
pub use transaction::{apply, dry_run, restore, ApplyResult, PatchReceipt};

use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
