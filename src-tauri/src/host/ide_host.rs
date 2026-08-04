use crate::host::app_host::{client_configuration_status, stop_app_for_reconfiguration};
use crate::host::process::{is_app_running, wait_for_app_state};
use host_integration::{
    discover, inspect_ide_settings, CodeSignatureVerifier, IdeSettingsState, InstallationState,
    MacOsCodeSignatureVerifier, PatchProfile,
};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

pub const ANTIGRAVITY_IDE_PATH: &str = "/Applications/Antigravity IDE.app";
pub const ANTIGRAVITY_IDE_BUNDLE_ID: &str = "com.google.antigravity-ide";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeStatus {
    pub installed: bool,
    pub compatible: bool,
    pub ide_running: bool,
    pub proxy_running: bool,
    pub state: &'static str,
    pub app_path: String,
    pub app_version: Option<String>,
    pub extension_version: Option<String>,
    pub extension_sha256: Option<String>,
    pub message: String,
    pub integration_state: &'static str,
    pub settings_path: String,
    pub integration_message: String,
    pub configuration_state: &'static str,
    pub configuration_message: String,
    pub can_enable_integration: bool,
    pub can_launch_ide: bool,
    pub can_disable_integration: bool,
}

pub fn discover_ide_sync(
    settings_path: &Path,
    integration_root: &Path,
    endpoint: &str,
    proxy_running: bool,
) -> Result<IdeStatus, String> {
    let profile = PatchProfile::antigravity_ide_2_1_1();
    let (integration_state, integration_message, can_disable_integration, settings_valid) =
        match inspect_ide_settings(settings_path, integration_root, endpoint) {
            Ok(status) => match status.state {
                IdeSettingsState::Disabled => (
                    "official",
                    format!("jetski.cloudCodeUrl 尚未指向当前本地代理 {endpoint}"),
                    false,
                    true,
                ),
                IdeSettingsState::Managed if status.endpoint_matches => (
                    "managed",
                    format!("jetski.cloudCodeUrl 已由 AGY BYOK 管理并指向当前本地代理 {endpoint}"),
                    true,
                    true,
                ),
                IdeSettingsState::Managed => (
                    "mismatch",
                    format!(
                        "jetski.cloudCodeUrl 仍由 AGY BYOK 管理，但尚未指向当前本地代理 {endpoint}；可重新设置或恢复官方模式"
                    ),
                    true,
                    true,
                ),
                IdeSettingsState::External if status.endpoint_matches => (
                    "external",
                    format!(
                        "当前相同 Endpoint {endpoint} 来自外部配置；可恢复官方模式"
                    ),
                    true,
                    true,
                ),
                IdeSettingsState::External => (
                    "mismatch",
                    "检测到 IDE 已配置其他本地代理地址，可重新设置为当前本地代理或恢复官方模式"
                        .to_string(),
                    true,
                    true,
                ),
            },
            Err(error) => ("conflict", error.to_string(), false, false),
        };

    let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
    if !app_path.is_dir() {
        return Ok(IdeStatus {
            installed: false,
            compatible: false,
            ide_running: false,
            proxy_running,
            state: "not_installed",
            app_path: ANTIGRAVITY_IDE_PATH.to_string(),
            app_version: None,
            extension_version: None,
            extension_sha256: None,
            message: "未在默认位置找到厂商 Antigravity IDE".to_string(),
            integration_state,
            settings_path: settings_path.display().to_string(),
            integration_message,
            configuration_state: "unavailable",
            configuration_message: "未找到应用".to_string(),
            can_enable_integration: false,
            can_launch_ide: false,
            can_disable_integration,
        });
    }

    let ide_running = is_app_running(app_path, "Antigravity IDE")?;
    let integration_message = {
        let message = if ide_running && integration_state == "mismatch" && can_disable_integration {
            format!("{integration_message}；更新或停用后将自动重启 Antigravity IDE")
        } else if ide_running && integration_state == "official" {
            format!("{integration_message}；启用后将自动重启 Antigravity IDE")
        } else if ide_running && integration_state == "managed" {
            format!("{integration_message}；停用后将自动重启 Antigravity IDE")
        } else {
            integration_message
        };
        if integration_state == "managed" && !proxy_running {
            format!("{message}；当前本地代理未运行")
        } else {
            message
        }
    };
    let installation = match discover(app_path, &profile.layout) {
        Ok(installation) => installation,
        Err(error) => {
            return Ok(IdeStatus {
                installed: true,
                compatible: false,
                ide_running,
                proxy_running,
                state: "incompatible",
                app_path: ANTIGRAVITY_IDE_PATH.to_string(),
                app_version: None,
                extension_version: None,
                extension_sha256: None,
                message: format!("无法识别当前 Antigravity IDE 安装：{error}"),
                integration_state,
                settings_path: settings_path.display().to_string(),
                integration_message,
                configuration_state: "unavailable",
                configuration_message: "当前版本暂时无法使用".to_string(),
                can_enable_integration: false,
                can_launch_ide: false,
                can_disable_integration,
            });
        }
    };
    let app_version = Some(installation.app_version.clone());
    let extension_version = Some(installation.extension_version.clone());
    let extension_sha256 = Some(installation.extension_sha256.clone());
    let (compatible, state, message) = match profile.classify(&installation) {
        Ok(InstallationState::VendorOriginal) => {
            match MacOsCodeSignatureVerifier
                .verify_vendor(&installation.app_path, &profile.bundle_id)
            {
                Ok(()) => (
                    true,
                    "vendor_original",
                    "厂商原版版本、哈希与 Google 签名匹配；不会被 AGY BYOK 修改".to_string(),
                ),
                Err(error) => (
                    false,
                    "modified",
                    format!("目标文件内容原始，但厂商签名不匹配：{error}"),
                ),
            }
        }
        Ok(InstallationState::PatchedByProfile) => (
            false,
            "patched",
            "厂商安装仍处于历史补丁状态；请重装原版后再启用代理模式".to_string(),
        ),
        Ok(InstallationState::Modified) => (
            false,
            "modified",
            "检测到未知修改，已禁止启用 IDE 代理模式".to_string(),
        ),
        Err(error) => (false, "incompatible", error.to_string()),
    };
    let integration_ready = matches!(integration_state, "managed" | "external");
    let (configuration_state, configuration_message) = client_configuration_status(
        integration_state,
        proxy_running,
        ide_running,
        app_path,
        endpoint,
    );
    let can_enable_integration = compatible
        && settings_valid
        && (matches!(integration_state, "official" | "mismatch" | "managed")
            || (integration_state == "external" && configuration_state == "needs_update"));
    let can_launch_ide = compatible
        && !ide_running
        && (integration_state == "official" || (integration_ready && proxy_running));

    Ok(IdeStatus {
        installed: true,
        compatible,
        ide_running,
        proxy_running,
        state,
        app_path: installation.app_path.display().to_string(),
        app_version,
        extension_version,
        extension_sha256,
        message,
        integration_state,
        settings_path: settings_path.display().to_string(),
        integration_message,
        configuration_state,
        configuration_message,
        can_enable_integration,
        can_launch_ide,
        can_disable_integration,
    })
}

pub fn stop_ide_for_reconfiguration(app_path: &Path, label: &str) -> Result<bool, String> {
    stop_app_for_reconfiguration(app_path, label)
}

pub fn restart_ide_app(app_path: &Path, label: &str) -> Result<(), String> {
    launch_ide_app()?;
    wait_for_app_state(app_path, label, true)
}

pub fn launch_ide_app() -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .env("TMPDIR", "/private/tmp")
        .arg(ANTIGRAVITY_IDE_PATH)
        .status()
        .map_err(|error| format!("无法启动 Antigravity IDE：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("启动 Antigravity IDE 失败：{status}"))
    }
}
