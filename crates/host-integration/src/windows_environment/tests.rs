use super::registry::{RegistryStringKind, RegistryStringValue};
use super::*;

#[test]
fn reconfiguration_keeps_the_first_saved_user_value() {
    let original = RegistryStringValue {
        value: "https://example.test".to_string(),
        kind: RegistryStringKind::String,
    };
    let receipt = WindowsEnvironmentReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        managed_endpoint: "http://127.0.0.1:51234".to_string(),
        original_cloud_code_url: Some(original.clone()),
        owners: WindowsEnvironmentOwners {
            app: false,
            cli: true,
        },
    };
    let current = RegistryStringValue {
        value: receipt.managed_endpoint.clone(),
        kind: RegistryStringKind::String,
    };

    assert!(receipt_matches_current_value(&receipt, Some(&current)));
    assert_eq!(receipt.original_cloud_code_url, Some(original));
}

#[test]
fn changed_user_value_is_not_considered_managed() {
    let receipt = WindowsEnvironmentReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        managed_endpoint: "http://127.0.0.1:51234".to_string(),
        original_cloud_code_url: None,
        owners: WindowsEnvironmentOwners {
            app: true,
            cli: true,
        },
    };
    let current = RegistryStringValue {
        value: "http://127.0.0.1:54321".to_string(),
        kind: RegistryStringKind::String,
    };

    assert!(!receipt_matches_current_value(&receipt, Some(&current)));
}

#[test]
fn owner_tracking_requires_the_last_owner_to_restore() {
    let mut owners = WindowsEnvironmentOwners {
        app: true,
        cli: true,
    };

    owners.remove(WindowsEnvironmentOwner::Cli);
    assert!(!owners.is_empty());
    owners.remove(WindowsEnvironmentOwner::App);
    assert!(owners.is_empty());
}
