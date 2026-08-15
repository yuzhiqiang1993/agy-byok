pub mod config_check;
pub mod host_check;
pub mod proxy_check;
pub mod types;

use crate::host::app_host::{self, restart_app};
use crate::host::ide_host::restart_ide;
use crate::state::{local_proxy_endpoint, DesktopState};
use host_integration::{enable_cli_integration, enable_ide_settings};
use std::time::{SystemTime, UNIX_EPOCH};

pub use types::{DiagnosticCategory, DiagnosticItem, DiagnosticLevel, DoctorReport, FixAction};

pub async fn run_diagnosis(state: &DesktopState) -> DoctorReport {
    let mut all_items = Vec::new();

    // 1. 本地代理检测
    all_items.extend(proxy_check::check_proxy(state).await);

    // 2. 配置与上游模型检测
    all_items.extend(config_check::check_config_and_providers(state).await);

    // 3. 宿主集成检测
    all_items.extend(host_check::check_hosts(state).await);

    // 计算总体健康度
    let overall_status = if all_items.iter().any(|i| i.level == DiagnosticLevel::Error) {
        DiagnosticLevel::Error
    } else if all_items
        .iter()
        .any(|i| i.level == DiagnosticLevel::Warning)
    {
        DiagnosticLevel::Warning
    } else {
        DiagnosticLevel::Pass
    };

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    DoctorReport {
        timestamp_ms,
        overall_status,
        items: all_items,
    }
}

pub async fn run_auto_fix(state: &DesktopState, action: FixAction) -> Result<DoctorReport, String> {
    let host_paths = state.current_host_paths();
    let root = &state.host_integration_root;

    let handle_guard = state.proxy_handle.lock().await;
    let actual_port = handle_guard
        .as_ref()
        .map(|h| h.local_addr().port())
        .unwrap_or_else(|| state.config_store.get_config().proxy_port);
    drop(handle_guard);

    let target_endpoint = local_proxy_endpoint(actual_port);

    match action {
        FixAction::StartProxy => {
            let _ = crate::commands::proxy::start_proxy_inner(state)
                .await
                .map_err(|e| format!("启动代理失败：{e}"))?;
        }
        FixAction::OpenAddProvider => {}
        FixAction::RepairIdeSettings => {
            let ide_paths = host_paths
                .ide
                .as_ref()
                .ok_or_else(|| "未找到 Antigravity IDE 安装路径".to_string())?;
            let settings_path = ide_paths
                .settings
                .as_ref()
                .ok_or_else(|| "未找到 Antigravity IDE settings.json 路径".to_string())?;
            enable_ide_settings(settings_path, root, &target_endpoint)
                .map_err(|e| e.to_string())?;
        }
        FixAction::RepairAppEnvironment => {
            let app_paths = host_paths
                .app
                .as_ref()
                .ok_or_else(|| "未找到 Antigravity App 安装路径".to_string())?;
            app_host::enable_integration(app_paths, root, &target_endpoint)?;
        }
        FixAction::RestartAppHost => {
            let app_paths = host_paths
                .app
                .as_ref()
                .ok_or_else(|| "未找到 Antigravity App 安装路径".to_string())?;
            let _ = app_host::enable_integration(app_paths, root, &target_endpoint);
            restart_app(app_paths, Some(&target_endpoint))?;
        }
        FixAction::RestartIdeHost => {
            let ide_paths = host_paths
                .ide
                .as_ref()
                .ok_or_else(|| "未找到 Antigravity IDE 安装路径".to_string())?;
            if let Some(settings_path) = ide_paths.settings.as_ref() {
                let _ = enable_ide_settings(settings_path, root, &target_endpoint);
            }
            restart_ide(ide_paths)?;
        }
        FixAction::PruneInvalidModels {
            provider_id,
            invalid_model_ids,
        } => {
            let invalid_set: std::collections::HashSet<_> = invalid_model_ids.into_iter().collect();
            state
                .config_store
                .update_config_with(|cfg| {
                    let removed_upstream_ids: std::collections::HashSet<String> = cfg
                        .upstream_models
                        .iter()
                        .filter(|um| {
                            um.provider_id == provider_id
                                && invalid_set.contains(&um.upstream_model_id)
                        })
                        .map(|um| um.id.clone())
                        .collect();

                    cfg.upstream_models
                        .retain(|um| !removed_upstream_ids.contains(&um.id));
                    cfg.virtual_models
                        .retain(|vm| !removed_upstream_ids.contains(&vm.upstream_model_id));
                })
                .map_err(|e| format!("更新配置失败：{e}"))?;
        }
        FixAction::EnableHostIntegration { host_type } => match host_type.as_str() {
            "ide" => {
                let ide_paths = host_paths
                    .ide
                    .as_ref()
                    .ok_or_else(|| "未找到 Antigravity IDE 安装路径".to_string())?;
                let settings_path = ide_paths
                    .settings
                    .as_ref()
                    .ok_or_else(|| "未找到 Antigravity IDE settings.json 路径".to_string())?;
                enable_ide_settings(settings_path, root, &target_endpoint)
                    .map_err(|e| e.to_string())?;
            }
            "app" => {
                let app_paths = host_paths
                    .app
                    .as_ref()
                    .ok_or_else(|| "未找到 Antigravity App 安装路径".to_string())?;
                app_host::enable_integration(app_paths, root, &target_endpoint)?;
            }
            "cli" => {
                enable_cli_integration(root, &target_endpoint).map_err(|e| e.to_string())?;
            }
            _ => return Err(format!("未知宿主类型：{host_type}")),
        },
    }

    // 修复完成后重新执行诊断，返回最新状态
    Ok(run_diagnosis(state).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agy_byok::domain::AppConfig;
    use agy_byok::proxy::ActivityLog;
    use agy_byok::storage::ConfigStore;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_doctor_diagnosis_initial_state() {
        let initial_config = AppConfig {
            proxy_port: 12345,
            ..AppConfig::default()
        };
        let config_store = ConfigStore::in_memory(initial_config);
        let root = std::path::PathBuf::from("/tmp/test_host_root");
        let state = DesktopState {
            config_store,
            host_integration_root: root,
            activity_log: Arc::new(ActivityLog::new()),
            proxy_host_mutation_lock: Mutex::new(()),
            proxy_handle: Mutex::new(None),
        };

        let report = run_diagnosis(&state).await;
        assert_eq!(report.overall_status, DiagnosticLevel::Error);
        assert!(!report.items.is_empty());
    }
}
