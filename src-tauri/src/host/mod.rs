pub mod app_host;
pub mod cli_host;
pub mod ide_host;
pub mod process;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientIntegrationState {
    Official,
    Managed,
    External,
    Mismatch,
    Conflict,
    Unavailable,
}

impl ClientIntegrationState {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Managed | Self::External)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientConfigurationState {
    NotEnabled,
    Matched,
    NotRunning,
    ServiceStopped,
    NeedsUpdate,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_state_codes_match_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(ClientIntegrationState::External).unwrap(),
            "external"
        );
        assert_eq!(
            serde_json::to_value(ClientConfigurationState::NeedsUpdate).unwrap(),
            "needs_update"
        );
    }
}
