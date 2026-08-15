use super::types::{DiagnosticCategory, DiagnosticItem, DiagnosticLevel, FixAction};
use crate::state::DesktopState;
use agy_byok::providers::fetch_provider_models;
use std::collections::{HashMap, HashSet};

pub async fn check_config_and_providers(state: &DesktopState) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    let config = state.config_store.get_config();

    // 1. 检查 Provider 数量
    let enabled_providers: Vec<_> = config.providers.iter().filter(|p| p.enabled).collect();
    if enabled_providers.is_empty() {
        items.push(DiagnosticItem {
            id: "config.no_enabled_providers".to_string(),
            category: DiagnosticCategory::Config,
            title: "未配置或未启用任何提供商".to_string(),
            message: "当前没有已启用的 Provider，所有模型请求将被拦截或直接失败。".to_string(),
            suggestion: None,
            level: DiagnosticLevel::Warning,
            auto_fixable: true,
            action: Some(FixAction::OpenAddProvider),
        });
    }

    // 2. 检查 VirtualModel 占位符冲突
    let mut host_id_counts: HashMap<&str, usize> = HashMap::new();
    for vm in config.virtual_models.iter().filter(|vm| vm.enabled) {
        if let Some(host_id) = &vm.host_model_id {
            *host_id_counts.entry(host_id.as_str()).or_default() += 1;
        }
    }
    let duplicate_host_ids: Vec<_> = host_id_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id.to_string())
        .collect();

    if !duplicate_host_ids.is_empty() {
        items.push(DiagnosticItem {
            id: "config.duplicate_host_ids".to_string(),
            category: DiagnosticCategory::Config,
            title: "虚拟模型宿主占位符 ID 冲突".to_string(),
            message: format!(
                "检测到多个虚拟模型分配了相同的宿主 ID：{}，这会导致路由混乱。",
                duplicate_host_ids.join(", ")
            ),
            suggestion: Some("建议重新保存提供商配置以重新分配互斥的占位符。".to_string()),
            level: DiagnosticLevel::Error,
            auto_fixable: false,
            action: None,
        });
    }

    // 3. 并发探测每个已启用的 Provider 连通性与模型有效性
    for provider in enabled_providers {
        let provider_upstreams: Vec<_> = config
            .upstream_models
            .iter()
            .filter(|um| um.provider_id == provider.id && um.enabled)
            .collect();

        if provider_upstreams.is_empty() {
            items.push(DiagnosticItem {
                id: format!("provider.{}.no_models", provider.id),
                category: DiagnosticCategory::Provider,
                title: format!("提供商「{}」未配置模型", provider.name),
                message: "该提供商已启用，但未配置任何上游模型。".to_string(),
                suggestion: Some("请在提供商卡片中点击【获取模型】并勾选需要的模型。".to_string()),
                level: DiagnosticLevel::Warning,
                auto_fixable: false,
                action: None,
            });
            continue;
        }

        // 尝试拉取上游实际模型目录
        match fetch_provider_models(provider).await {
            Ok(catalog) => {
                let available_model_ids: HashSet<String> =
                    catalog.into_iter().map(|m| m.id).collect();

                let mut invalid_models = Vec::new();
                for um in &provider_upstreams {
                    if !available_model_ids.contains(&um.upstream_model_id) {
                        invalid_models.push(um.upstream_model_id.clone());
                    }
                }

                if !invalid_models.is_empty() {
                    items.push(DiagnosticItem {
                        id: format!("provider.{}.invalid_models", provider.id),
                        category: DiagnosticCategory::Provider,
                        title: format!("提供商「{}」存在失效/未部署的模型", provider.name),
                        message: format!(
                            "上游当前未提供以下模型：{}。调用这些模型会直接导致 400 错误。",
                            invalid_models.join(", ")
                        ),
                        suggestion: Some("建议点击【一键清理失效模型】移除不存在的条目。".to_string()),
                        level: DiagnosticLevel::Error,
                        auto_fixable: true,
                        action: Some(FixAction::PruneInvalidModels {
                            provider_id: provider.id.clone(),
                            invalid_model_ids: invalid_models,
                        }),
                    });
                } else {
                    items.push(DiagnosticItem {
                        id: format!("provider.{}.healthy", provider.id),
                        category: DiagnosticCategory::Provider,
                        title: format!("提供商「{}」连通正常", provider.name),
                        message: format!(
                            "鉴权成功，已配置的 {} 个模型均存在于上游目录中。",
                            provider_upstreams.len()
                        ),
                        suggestion: None,
                        level: DiagnosticLevel::Pass,
                        auto_fixable: false,
                        action: None,
                    });
                }
            }
            Err(err) => {
                items.push(DiagnosticItem {
                    id: format!("provider.{}.unreachable", provider.id),
                    category: DiagnosticCategory::Provider,
                    title: format!("提供商「{}」连通失败", provider.name),
                    message: format!("无法连接上游端点或鉴权失败：{}", err.message),
                    suggestion: Some("请检查提供商的 API Key、网络代理或 Endpoint 地址是否正确。".to_string()),
                    level: DiagnosticLevel::Error,
                    auto_fixable: false,
                    action: None,
                });
            }
        }
    }

    items
}
