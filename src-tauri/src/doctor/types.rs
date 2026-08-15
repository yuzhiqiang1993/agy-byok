use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Pass,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Proxy,
    Config,
    Provider,
    Host,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FixAction {
    StartProxy,
    OpenAddProvider,
    RepairIdeSettings,
    RepairAppEnvironment,
    RestartAppHost,
    RestartIdeHost,
    PruneInvalidModels {
        provider_id: String,
        invalid_model_ids: Vec<String>,
    },
    EnableHostIntegration {
        host_type: String, // "ide" | "app" | "cli"
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItem {
    pub id: String,
    pub category: DiagnosticCategory,
    pub title: String,
    pub message: String,
    pub suggestion: Option<String>,
    pub level: DiagnosticLevel,
    pub auto_fixable: bool,
    pub action: Option<FixAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub timestamp_ms: u64,
    pub overall_status: DiagnosticLevel,
    pub items: Vec<DiagnosticItem>,
}
