use agy_byok::proxy::{HttpServerHandle, HttpServerOptions, LoopbackHttpServer, ProxyServer};
use agy_byok::storage::{default_config_path, AppConfig, ConfigStore};
use host_integration::{
    create_managed_copy, discover, dry_run, inspect_managed_copy, remove_managed_copy,
    restore as restore_patch, sha256, CodeSignatureVerifier, InstallationState,
    MacOsCodeSignatureVerifier, PatchProfile, PatchReceipt, PatchTransactionState,
    MANAGED_APP_NAME, MANAGED_RECEIPT_FILE,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

const PROXY_PORT: u16 = 50999;
const OFFICIAL_CLOUD_CODE_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
const ANTIGRAVITY_IDE_PATH: &str = "/Applications/Antigravity IDE.app";

struct DesktopState {
    config_store: ConfigStore,
    snapshot_root: PathBuf,
    managed_root: PathBuf,
    proxy_handle: Mutex<Option<HttpServerHandle>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStatus {
    state: &'static str,
    address: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdeStatus {
    installed: bool,
    compatible: bool,
    can_dry_run: bool,
    can_restore: bool,
    receipt_path: Option<String>,
    state: &'static str,
    app_path: String,
    app_version: Option<String>,
    extension_version: Option<String>,
    extension_sha256: Option<String>,
    message: String,
    managed_state: &'static str,
    managed_app_path: String,
    managed_receipt_path: Option<String>,
    managed_message: String,
    can_create_managed: bool,
    can_launch_managed: bool,
    can_remove_managed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DryRunResult {
    profile_id: String,
    endpoint: String,
    candidate_sha256: String,
}

#[tauri::command]
fn get_config(state: State<'_, DesktopState>) -> AppConfig {
    state.config_store.get_config()
}

#[tauri::command]
fn save_config(config: AppConfig, state: State<'_, DesktopState>) -> Result<AppConfig, String> {
    state.config_store.update_config(config)?;
    Ok(state.config_store.get_config())
}

#[tauri::command]
async fn proxy_status(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await;
    Ok(status_from_handle(handle.as_ref()))
}

#[tauri::command]
async fn start_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let mut handle = state.proxy_handle.lock().await;
    if handle.is_some() {
        return Ok(status_from_handle(handle.as_ref()));
    }

    let server = Arc::new(ProxyServer::new(state.config_store.clone(), PROXY_PORT));
    let options = HttpServerOptions {
        official_cloud_code_endpoint: Some(OFFICIAL_CLOUD_CODE_ENDPOINT.to_string()),
        ..HttpServerOptions::default()
    };
    let started = LoopbackHttpServer::start(server, options)
        .await
        .map_err(|error| error.to_string())?;
    *handle = Some(started);
    Ok(status_from_handle(handle.as_ref()))
}

#[tauri::command]
async fn stop_proxy(state: State<'_, DesktopState>) -> Result<ProxyStatus, String> {
    let handle = state.proxy_handle.lock().await.take();
    if let Some(handle) = handle {
        handle.shutdown().await.map_err(|error| error.to_string())?;
    }
    Ok(ProxyStatus {
        state: "stopped",
        address: None,
    })
}

#[tauri::command]
async fn discover_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let snapshot_root = state.snapshot_root.clone();
    let managed_root = state.managed_root.clone();
    tauri::async_runtime::spawn_blocking(move || discover_ide_sync(&snapshot_root, &managed_root))
        .await
        .map_err(|error| format!("IDE discovery task failed: {error}"))?
}

#[tauri::command]
async fn dry_run_ide() -> Result<DryRunResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let profile = PatchProfile::antigravity_ide_2_1_1();
        let candidate =
            dry_run(ANTIGRAVITY_IDE_PATH, &profile).map_err(|error| error.to_string())?;
        Ok(DryRunResult {
            profile_id: profile.id,
            endpoint: profile.endpoint,
            candidate_sha256: sha256(candidate.as_bytes()),
        })
    })
    .await
    .map_err(|error| format!("IDE Dry Run task failed: {error}"))?
}

#[tauri::command]
async fn create_managed_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let snapshot_root = state.snapshot_root.clone();
    let managed_root = state.managed_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_ide_not_running(&managed_root)?;
        let profile = PatchProfile::antigravity_ide_2_1_1();
        create_managed_copy(ANTIGRAVITY_IDE_PATH, &managed_root, &profile)
            .map_err(|error| error.to_string())?;
        discover_ide_sync(&snapshot_root, &managed_root)
    })
    .await
    .map_err(|error| format!("managed IDE creation task failed: {error}"))?
}

#[tauri::command]
async fn launch_managed_ide(state: State<'_, DesktopState>) -> Result<(), String> {
    if state.proxy_handle.lock().await.is_none() {
        return Err("请先启动 AGY BYOK 本地代理，再打开托管 IDE".to_string());
    }
    let managed_root = state.managed_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let profile = PatchProfile::antigravity_ide_2_1_1();
        let receipt = inspect_managed_copy(&managed_root, &profile)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "尚未创建可启动的托管 IDE".to_string())?;
        let status = Command::new("/usr/bin/open")
            .env("TMPDIR", "/private/tmp")
            .arg("-n")
            .arg(&receipt.managed_app_path)
            .status()
            .map_err(|error| format!("无法启动托管 IDE：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("启动托管 IDE 失败：{status}"))
        }
    })
    .await
    .map_err(|error| format!("managed IDE launch task failed: {error}"))?
}

#[tauri::command]
async fn remove_managed_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let snapshot_root = state.snapshot_root.clone();
    let managed_root = state.managed_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_managed_ide_not_running(&managed_root)?;
        let profile = PatchProfile::antigravity_ide_2_1_1();
        remove_managed_copy(&managed_root, &profile).map_err(|error| error.to_string())?;
        discover_ide_sync(&snapshot_root, &managed_root)
    })
    .await
    .map_err(|error| format!("managed IDE removal task failed: {error}"))?
}

#[tauri::command]
async fn restore_ide(state: State<'_, DesktopState>) -> Result<IdeStatus, String> {
    let snapshot_root = state.snapshot_root.clone();
    let managed_root = state.managed_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_ide_not_running(&managed_root)?;
        let profile = PatchProfile::antigravity_ide_2_1_1();
        let receipt_path =
            find_active_receipt(&snapshot_root, &profile, Path::new(ANTIGRAVITY_IDE_PATH))?
                .map(|(path, _)| path)
                .ok_or_else(|| "没有找到可用于恢复的 AGY BYOK Receipt".to_string())?;
        restore_patch(
            ANTIGRAVITY_IDE_PATH,
            &profile,
            &receipt_path,
            &MacOsCodeSignatureVerifier,
        )
        .map_err(|error| error.to_string())?;
        discover_ide_sync(&snapshot_root, &managed_root)
    })
    .await
    .map_err(|error| format!("IDE Restore task failed: {error}"))?
}

fn status_from_handle(handle: Option<&HttpServerHandle>) -> ProxyStatus {
    match handle {
        Some(handle) => ProxyStatus {
            state: "running",
            address: Some(handle.local_addr().to_string()),
        },
        None => ProxyStatus {
            state: "stopped",
            address: None,
        },
    }
}

fn discover_ide_sync(snapshot_root: &Path, managed_root: &Path) -> Result<IdeStatus, String> {
    let profile = PatchProfile::antigravity_ide_2_1_1();
    let managed_app_path = managed_root.join(MANAGED_APP_NAME);
    let (
        managed_state,
        managed_receipt_path,
        managed_message,
        can_launch_managed,
        can_remove_managed,
    ) = match inspect_managed_copy(managed_root, &profile) {
        Ok(Some(_)) => (
            "ready",
            Some(
                managed_root
                    .join(MANAGED_RECEIPT_FILE)
                    .display()
                    .to_string(),
            ),
            "托管副本已就绪；启动前请确认本地代理正在运行".to_string(),
            true,
            true,
        ),
        Ok(None) => (
            "not_created",
            None,
            "尚未创建托管副本".to_string(),
            false,
            false,
        ),
        Err(error) => (
            "invalid",
            None,
            format!("托管副本状态异常：{error}"),
            false,
            false,
        ),
    };

    let app_path = Path::new(ANTIGRAVITY_IDE_PATH);
    if !app_path.is_dir() {
        return Ok(IdeStatus {
            installed: false,
            compatible: false,
            can_dry_run: false,
            can_restore: false,
            receipt_path: None,
            state: "not_installed",
            app_path: ANTIGRAVITY_IDE_PATH.to_string(),
            app_version: None,
            extension_version: None,
            extension_sha256: None,
            message: "未在默认位置找到厂商 Antigravity IDE".to_string(),
            managed_state,
            managed_app_path: managed_app_path.display().to_string(),
            managed_receipt_path,
            managed_message,
            can_create_managed: false,
            can_launch_managed,
            can_remove_managed,
        });
    }

    let installation = discover(app_path, &profile.layout).map_err(|error| error.to_string())?;
    let app_version = Some(installation.app_version.clone());
    let extension_version = Some(installation.extension_version.clone());
    let extension_sha256 = Some(installation.extension_sha256.clone());
    let active_receipt = find_active_receipt(snapshot_root, &profile, &installation.app_path)?;
    let receipt_path = active_receipt
        .as_ref()
        .map(|(path, _)| path.display().to_string());

    let (compatible, can_dry_run, can_restore, state, message) =
        match profile.classify(&installation) {
            Ok(InstallationState::VendorOriginal) => {
                match MacOsCodeSignatureVerifier
                    .verify_vendor(&installation.app_path, &profile.bundle_id)
                {
                    Ok(()) => (
                        true,
                        true,
                        false,
                        "vendor_original",
                        "厂商原版版本、哈希与 Google 签名匹配；不会被 AGY BYOK 修改".to_string(),
                    ),
                    Err(error) => (
                        false,
                        false,
                        false,
                        "modified",
                        format!("目标文件内容原始，但厂商签名不匹配：{error}"),
                    ),
                }
            }
            Ok(InstallationState::PatchedByProfile) => {
                let can_restore = active_receipt.as_ref().is_some_and(|(_, receipt)| {
                    receipt.executable_sha256 == installation.executable_sha256
                });
                (
                    true,
                    false,
                    can_restore,
                    "patched",
                    if can_restore {
                        "厂商安装仍处于历史补丁状态，可以使用 Receipt 恢复".to_string()
                    } else {
                        "厂商安装包含历史补丁，但缺少匹配的 Applied Receipt".to_string()
                    },
                )
            }
            Ok(InstallationState::Modified) => (
                true,
                false,
                false,
                "modified",
                "检测到未知修改，已禁止创建托管副本和历史恢复".to_string(),
            ),
            Err(error) => (false, false, false, "incompatible", error.to_string()),
        };
    let can_create_managed = compatible && managed_state == "not_created";

    Ok(IdeStatus {
        installed: true,
        compatible,
        can_dry_run,
        can_restore,
        receipt_path,
        state,
        app_path: installation.app_path.display().to_string(),
        app_version,
        extension_version,
        extension_sha256,
        message,
        managed_state,
        managed_app_path: managed_app_path.display().to_string(),
        managed_receipt_path,
        managed_message,
        can_create_managed,
        can_launch_managed,
        can_remove_managed,
    })
}

fn ensure_ide_not_running(managed_root: &Path) -> Result<(), String> {
    ensure_app_not_running(Path::new(ANTIGRAVITY_IDE_PATH), "厂商 Antigravity IDE")?;
    ensure_managed_ide_not_running(managed_root)
}

fn ensure_managed_ide_not_running(managed_root: &Path) -> Result<(), String> {
    ensure_app_not_running(&managed_root.join(MANAGED_APP_NAME), "AGY BYOK 托管 IDE")
}

fn ensure_app_not_running(app_path: &Path, label: &str) -> Result<(), String> {
    let executable = app_path.join("Contents/MacOS/Electron");
    let pattern = format!("^{}( |$)", executable.display());
    let status = Command::new("pgrep")
        .args(["-f", &pattern])
        .status()
        .map_err(|error| format!("无法检查 {label} 进程：{error}"))?;
    match status.code() {
        Some(1) => Ok(()),
        Some(0) => Err(format!("请先完全退出 {label}")),
        _ => Err(format!("检查 {label} 进程失败：{status}")),
    }
}

fn find_active_receipt(
    snapshot_root: &Path,
    profile: &PatchProfile,
    app_path: &Path,
) -> Result<Option<(PathBuf, PatchReceipt)>, String> {
    if !snapshot_root.is_dir() {
        return Ok(None);
    }

    let mut latest: Option<(u128, PathBuf, PatchReceipt)> = None;
    let entries =
        fs::read_dir(snapshot_root).map_err(|error| format!("无法读取 Snapshot 目录：{error}"))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let receipt_path = entry.path().join("receipt.json");
        let bytes = match fs::read(&receipt_path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let receipt: PatchReceipt = match serde_json::from_slice(&bytes) {
            Ok(receipt) => receipt,
            Err(_) => continue,
        };
        if receipt.profile_id != profile.id
            || receipt.app_path != app_path
            || receipt.state != PatchTransactionState::Applied
        {
            continue;
        }
        let Some(applied_at) = receipt.applied_at_unix_ms else {
            continue;
        };
        if latest
            .as_ref()
            .is_none_or(|(timestamp, _, _)| applied_at > *timestamp)
        {
            latest = Some((applied_at, receipt_path, receipt));
        }
    }
    Ok(latest.map(|(_, path, receipt)| (path, receipt)))
}

fn create_state() -> Result<DesktopState, String> {
    let config_path = default_config_path()?;
    let config_exists = config_path.exists();
    let snapshot_root = config_path
        .parent()
        .ok_or_else(|| "AGY BYOK 配置路径缺少父目录".to_string())?
        .join("host-snapshots");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位用户主目录，不能创建托管 IDE".to_string())?;
    if !home.is_absolute() {
        return Err("用户主目录不是绝对路径，不能创建托管 IDE".to_string());
    }
    let managed_root = home.join("Applications/AGY BYOK");
    let config_store = ConfigStore::load_from_file(&config_path)?;
    if !config_exists {
        config_store.update_config(config_store.get_config())?;
    }

    Ok(DesktopState {
        config_store,
        snapshot_root,
        managed_root,
        proxy_handle: Mutex::new(None),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = create_state().expect("failed to initialize AGY BYOK desktop state");
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            proxy_status,
            start_proxy,
            stop_proxy,
            discover_ide,
            dry_run_ide,
            create_managed_ide,
            launch_managed_ide,
            remove_managed_ide,
            restore_ide
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGY BYOK");
}
