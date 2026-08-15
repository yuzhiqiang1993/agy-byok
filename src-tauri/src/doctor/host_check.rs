use super::types::{DiagnosticCategory, DiagnosticItem, DiagnosticLevel, FixAction};
use crate::host::app_host::discover_app_sync;
use crate::host::cli_host::discover_cli_sync;
use crate::host::ide_host::discover_ide_sync;
use crate::host::ClientIntegrationState;
use crate::state::{local_proxy_endpoint, DesktopState};

pub async fn check_hosts(state: &DesktopState) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    let host_paths = state.current_host_paths();
    let root = &state.host_integration_root;

    let handle_guard = state.proxy_handle.lock().await;
    let actual_port = handle_guard
        .as_ref()
        .map(|h| h.local_addr().port())
        .unwrap_or_else(|| state.config_store.get_config().proxy_port);
    let proxy_running = handle_guard.is_some();
    drop(handle_guard);

    let target_endpoint = local_proxy_endpoint(actual_port);

    // ==========================================
    // 1. Antigravity IDE 诊断
    // ==========================================
    match discover_ide_sync(
        host_paths.ide.as_ref(),
        root,
        &target_endpoint,
        proxy_running,
    ) {
        Ok(ide_status) if ide_status.installed => {
            match ide_status.integration_state {
                ClientIntegrationState::Managed => {
                    items.push(DiagnosticItem {
                        id: "host.ide.healthy".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 代理接入正常".to_string(),
                        message: format!("settings.json 已正确配置为 {target_endpoint}。"),
                        suggestion: None,
                        level: DiagnosticLevel::Pass,
                        auto_fixable: false,
                        action: None,
                    });
                }
                ClientIntegrationState::Official => {
                    // 使用官方模式属于正常直接可用的状态，仅作为信息项（Info），不影响整体健康度
                    items.push(DiagnosticItem {
                        id: "host.ide.official".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 使用官方模式（未接入代理）".to_string(),
                        message: "当前直连 Google 官方服务，可直接正常使用。如需在 IDE 中使用自定义模型，可在【运行概览】中启用代理接入。".to_string(),
                        suggestion: None,
                        level: DiagnosticLevel::Info,
                        auto_fixable: false,
                        action: None,
                    });
                }
                ClientIntegrationState::Mismatch => {
                    items.push(DiagnosticItem {
                        id: "host.ide.port_mismatch".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 端口与当前代理不一致".to_string(),
                        message: format!(
                            "IDE 当前指向旧端点，如需使用当前代理需要更新为：{target_endpoint}。"
                        ),
                        suggestion: Some(
                            "点击【一键修复】即可自动修正 settings.json 配置。".to_string(),
                        ),
                        level: DiagnosticLevel::Warning,
                        auto_fixable: true,
                        action: Some(FixAction::RepairIdeSettings),
                    });
                }
                ClientIntegrationState::External => {
                    items.push(DiagnosticItem {
                        id: "host.ide.external".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 由外部配置管理".to_string(),
                        message: format!(
                            "settings.json 已正确指向 {target_endpoint}（外部托管）。"
                        ),
                        suggestion: None,
                        level: DiagnosticLevel::Pass,
                        auto_fixable: false,
                        action: None,
                    });
                }
                ClientIntegrationState::Conflict => {
                    items.push(DiagnosticItem {
                        id: "host.ide.conflict".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 配置文件存在冲突".to_string(),
                        message: "settings.json 中存在重复或异常的 cloudCodeUrl 配置。".to_string(),
                        suggestion: Some("建议点击【一键修复】重置 IDE 配置。".to_string()),
                        level: DiagnosticLevel::Error,
                        auto_fixable: true,
                        action: Some(FixAction::RepairIdeSettings),
                    });
                }
                ClientIntegrationState::Unavailable => {
                    items.push(DiagnosticItem {
                        id: "host.ide.unavailable".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity IDE 配置文件不可用".to_string(),
                        message: "无法访问 settings.json 配置文件。".to_string(),
                        suggestion: None,
                        level: DiagnosticLevel::Warning,
                        auto_fixable: false,
                        action: None,
                    });
                }
            }
        }
        Ok(_) => {}
        Err(err) => {
            items.push(DiagnosticItem {
                id: "host.ide.inspect_error".to_string(),
                category: DiagnosticCategory::Host,
                title: "无法读取 Antigravity IDE 配置".to_string(),
                message: format!("检查 settings.json 失败：{err}"),
                suggestion: Some("请检查 IDE 配置目录权限。".to_string()),
                level: DiagnosticLevel::Warning,
                auto_fixable: false,
                action: None,
            });
        }
    }

    // ==========================================
    // 2. Antigravity App 诊断
    // ==========================================
    match discover_app_sync(
        host_paths.app.as_ref(),
        root,
        &target_endpoint,
        proxy_running,
    ) {
        Ok(app_status) if app_status.installed => {
            match app_status.integration_state {
                ClientIntegrationState::Managed | ClientIntegrationState::External => {
                    let running_hint = if app_status.app_running {
                        "（App 正在运行）"
                    } else {
                        "（App 未启动）"
                    };
                    items.push(DiagnosticItem {
                        id: "host.app.healthy".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity App 代理接入正常".to_string(),
                        message: format!("环境变量 CLOUD_CODE_URL 已正确配置为 {target_endpoint} {running_hint}。"),
                        suggestion: None,
                        level: DiagnosticLevel::Pass,
                        auto_fixable: false,
                        action: None,
                    });
                }
                ClientIntegrationState::Official => {
                    // 使用官方模式属于正常直接可用的状态，作为信息项（Info）
                    items.push(DiagnosticItem {
                        id: "host.app.official".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity App 使用官方模式（未接入代理）".to_string(),
                        message: "当前直连 Google 官方服务，可直接正常使用。如需在 App 中使用自定义模型，可在【运行概览】中启用代理接入。".to_string(),
                        suggestion: None,
                        level: DiagnosticLevel::Info,
                        auto_fixable: false,
                        action: None,
                    });
                }
                ClientIntegrationState::Mismatch => {
                    items.push(DiagnosticItem {
                        id: "host.app.port_mismatch".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity App 环境变量端口与当前代理不一致".to_string(),
                        message: format!("会话变量 CLOUD_CODE_URL 指向旧端口，当前代理监听在 {target_endpoint}。"),
                        suggestion: Some("点击【一键修复】即可更新环境变量。".to_string()),
                        level: DiagnosticLevel::Warning,
                        auto_fixable: true,
                        action: Some(FixAction::RepairAppEnvironment),
                    });
                }
                ClientIntegrationState::Conflict | ClientIntegrationState::Unavailable => {
                    items.push(DiagnosticItem {
                        id: "host.app.conflict".to_string(),
                        category: DiagnosticCategory::Host,
                        title: "Antigravity App 环境异常".to_string(),
                        message: "无法正常读取或管理 App 环境变量。".to_string(),
                        suggestion: None,
                        level: DiagnosticLevel::Warning,
                        auto_fixable: false,
                        action: None,
                    });
                }
            }
        }
        Ok(_) => {}
        Err(err) => {
            items.push(DiagnosticItem {
                id: "host.app.inspect_error".to_string(),
                category: DiagnosticCategory::Host,
                title: "无法读取 Antigravity App 状态".to_string(),
                message: format!("检查 App 失败：{err}"),
                suggestion: None,
                level: DiagnosticLevel::Warning,
                auto_fixable: false,
                action: None,
            });
        }
    }

    // ==========================================
    // 3. Antigravity CLI 诊断
    // ==========================================
    match discover_cli_sync(root, &target_endpoint, proxy_running) {
        Ok(cli_status) if cli_status.installed => {
            if cli_status.integration_state == ClientIntegrationState::Managed
                || cli_status.integration_state == ClientIntegrationState::External
            {
                items.push(DiagnosticItem {
                    id: "host.cli.healthy".to_string(),
                    category: DiagnosticCategory::Host,
                    title: "Antigravity CLI 代理接入正常".to_string(),
                    message: format!("CLI 环境变量已配置为 {target_endpoint}。"),
                    suggestion: None,
                    level: DiagnosticLevel::Pass,
                    auto_fixable: false,
                    action: None,
                });
            } else {
                // CLI 未启用代理属于正常状态，作为信息项（Info）
                items.push(DiagnosticItem {
                    id: "host.cli.official".to_string(),
                    category: DiagnosticCategory::Host,
                    title: "Antigravity CLI 使用官方模式（未接入代理）".to_string(),
                    message: "当前在终端执行 agy 命令行直连官方服务，可直接正常使用。如需使用自定义模型，可在【运行概览】中启用代理接入。".to_string(),
                    suggestion: None,
                    level: DiagnosticLevel::Info,
                    auto_fixable: false,
                    action: None,
                });
            }
        }
        _ => {}
    }

    items
}
