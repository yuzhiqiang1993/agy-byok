use super::receipt::MacOsEnvironmentOwners;
use super::*;

#[test]
fn owner_tracking_restores_only_after_the_last_owner() {
    let mut owners = MacOsEnvironmentOwners::with(MacOsEnvironmentOwner::App);
    owners.insert(MacOsEnvironmentOwner::Cli);

    owners.remove(MacOsEnvironmentOwner::App);
    assert!(!owners.is_empty());
    owners.remove(MacOsEnvironmentOwner::Cli);
    assert!(owners.is_empty());
}

#[test]
fn environment_status_requires_both_ownership_and_current_value() {
    let owners = MacOsEnvironmentOwners::with(MacOsEnvironmentOwner::App);
    let managed = MacOsEnvironmentStatus {
        configured_endpoint: Some("http://127.0.0.1:51234".to_string()),
        current_value_is_managed: true,
        owners: owners.clone(),
    };
    let external = MacOsEnvironmentStatus {
        configured_endpoint: Some("http://127.0.0.1:51234".to_string()),
        current_value_is_managed: false,
        owners,
    };

    assert!(managed.is_active_for(MacOsEnvironmentOwner::App));
    assert!(!managed.is_active_for(MacOsEnvironmentOwner::Cli));
    assert!(!external.is_active_for(MacOsEnvironmentOwner::App));
}

#[test]
#[ignore = "changes the current macOS launchd environment and is intended for local smoke tests"]
fn launchctl_ownership_round_trip() {
    struct EnvironmentGuard(Option<String>);

    impl Drop for EnvironmentGuard {
        fn drop(&mut self) {
            match self.0.as_deref() {
                Some(value) => super::launchctl::set_endpoint(value).unwrap(),
                None => super::launchctl::remove_endpoint().unwrap(),
            }
        }
    }

    let original = super::launchctl::read_endpoint().unwrap();
    let _guard = EnvironmentGuard(original.clone());
    let directory = tempfile::tempdir().unwrap();
    let endpoint = "http://127.0.0.1:51234";

    let app_enabled = enable(directory.path(), MacOsEnvironmentOwner::App, endpoint).unwrap();
    assert!(app_enabled.is_active_for(MacOsEnvironmentOwner::App));

    let both_enabled = enable(directory.path(), MacOsEnvironmentOwner::Cli, endpoint).unwrap();
    assert!(both_enabled.is_active_for(MacOsEnvironmentOwner::App));
    assert!(both_enabled.is_active_for(MacOsEnvironmentOwner::Cli));

    let cli_only = disable(directory.path(), MacOsEnvironmentOwner::App).unwrap();
    assert!(!cli_only.has_owner(MacOsEnvironmentOwner::App));
    assert!(cli_only.is_active_for(MacOsEnvironmentOwner::Cli));

    let restored = disable(directory.path(), MacOsEnvironmentOwner::Cli).unwrap();
    assert_eq!(restored.configured_endpoint, original);
}
