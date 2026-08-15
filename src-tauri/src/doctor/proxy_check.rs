use super::types::{DiagnosticCategory, DiagnosticItem, DiagnosticLevel, FixAction};
use crate::state::DesktopState;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

pub async fn check_proxy(state: &DesktopState) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    let config = state.config_store.get_config();
    let configured_port = config.proxy_port;

    let handle_guard = state.proxy_handle.lock().await;
    let running_handle = handle_guard.as_ref();

    if running_handle.is_none() {
        items.push(DiagnosticItem {
            id: "proxy.not_running".to_string(),
            category: DiagnosticCategory::Proxy,
            title: "本地代理服务未运行".to_string(),
            message: format!(
                "代理服务处于停止状态，无法拦截转发请求（配置端口：{configured_port}）。"
            ),
            suggestion: Some("请启动本地代理服务。".to_string()),
            level: DiagnosticLevel::Error,
            auto_fixable: true,
            action: Some(FixAction::StartProxy),
        });
        return items;
    }

    let actual_port = running_handle.unwrap().local_addr().port();
    drop(handle_guard);

    if actual_port != configured_port {
        items.push(DiagnosticItem {
            id: "proxy.port_mismatch".to_string(),
            category: DiagnosticCategory::Proxy,
            title: "代理实际端口与配置端口不一致".to_string(),
            message: format!(
                "配置端口为 {configured_port}，但实际监听在 {actual_port}（可能因首选端口被占用）。"
            ),
            suggestion: Some("宿主接入将自动使用实际端口，但建议释放首选端口。".to_string()),
            level: DiagnosticLevel::Warning,
            auto_fixable: false,
            action: None,
        });
    }

    // 检查本地 TCP 端口连通性
    let addr = format!("127.0.0.1:{actual_port}");
    match timeout(Duration::from_millis(1500), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => {
            items.push(DiagnosticItem {
                id: "proxy.healthy".to_string(),
                category: DiagnosticCategory::Proxy,
                title: "本地代理服务运行正常".to_string(),
                message: format!("代理已就绪并正常监听 http://127.0.0.1:{actual_port}。"),
                suggestion: None,
                level: DiagnosticLevel::Pass,
                auto_fixable: false,
                action: None,
            });
        }
        Ok(Err(err)) => {
            items.push(DiagnosticItem {
                id: "proxy.unreachable".to_string(),
                category: DiagnosticCategory::Proxy,
                title: "本地代理端点无法连通".to_string(),
                message: format!("连接 127.0.0.1:{actual_port} 失败：{err}"),
                suggestion: Some("请检查本地网络权限或重启代理。".to_string()),
                level: DiagnosticLevel::Error,
                auto_fixable: true,
                action: Some(FixAction::StartProxy),
            });
        }
        Err(_) => {
            items.push(DiagnosticItem {
                id: "proxy.timeout".to_string(),
                category: DiagnosticCategory::Proxy,
                title: "本地代理端点连接超时".to_string(),
                message: format!("尝试连接 127.0.0.1:{actual_port} 超时。"),
                suggestion: Some("建议重启代理服务。".to_string()),
                level: DiagnosticLevel::Error,
                auto_fixable: true,
                action: Some(FixAction::StartProxy),
            });
        }
    }

    items
}
