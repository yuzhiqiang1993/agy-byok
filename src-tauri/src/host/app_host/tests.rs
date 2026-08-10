use super::*;

#[test]
fn configuration_state_is_derived_from_structured_status() {
    assert_eq!(
        configuration_state(ClientIntegrationState::Managed, true, true, true),
        ClientConfigurationState::Matched
    );
    assert_eq!(
        configuration_state(ClientIntegrationState::Managed, false, true, true),
        ClientConfigurationState::NeedsUpdate
    );
    assert_eq!(
        configuration_state(ClientIntegrationState::External, true, false, true),
        ClientConfigurationState::ServiceStopped
    );
}

#[test]
fn environment_ownership_is_mapped_consistently_on_both_platforms() {
    let target = "http://127.0.0.1:51234";
    let managed = environment_integration_details(Some(target), true, true, target);
    let external = environment_integration_details(Some(target), false, false, target);
    let changed =
        environment_integration_details(Some("http://127.0.0.1:54321"), false, true, target);

    let official = environment_integration_details(None, false, false, target);

    assert_eq!(managed.state, ClientIntegrationState::Managed);
    assert!(managed.can_disable);
    assert_eq!(external.state, ClientIntegrationState::External);
    assert!(external.can_disable);
    assert_eq!(changed.state, ClientIntegrationState::Mismatch);
    assert!(changed.can_disable);
    assert_eq!(official.state, ClientIntegrationState::Official);
    assert!(!official.can_disable);
}
