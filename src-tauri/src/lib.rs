mod commands;
mod host;
mod native_ui;
mod platform;
mod state;

use native_ui::NativeLocale;
use state::create_state;
pub use state::DesktopState;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Runtime,
};

#[cfg(desktop)]
const MAIN_TRAY_ID: &str = "main";

#[cfg(desktop)]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn tray_menu<R: Runtime, M: tauri::Manager<R>>(
    manager: &M,
    locale: NativeLocale,
) -> tauri::Result<Menu<R>> {
    let show_item = MenuItem::with_id(manager, "show", locale.tray_show(), true, None::<&str>)?;
    let quit_item = MenuItem::with_id(manager, "quit", locale.tray_quit(), true, None::<&str>)?;
    Menu::with_items(manager, &[&show_item, &quit_item])
}

#[cfg(desktop)]
fn setup_tray(app: &mut tauri::App, locale: NativeLocale) -> tauri::Result<()> {
    let menu = tray_menu(app, locale)?;

    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("./icons/tray-icon-solid.png");
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::include_image!("./icons/tray-icon-solid-color.png");

    TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("AGY BYOK")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg(desktop)]
pub(crate) fn set_tray_locale(app: &AppHandle, locale: NativeLocale) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        tray.set_menu(Some(tray_menu(app, locale)?))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    let builder = tauri::Builder::default();

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main_window(app);
    }));

    let builder =
        builder
            .plugin(tauri_plugin_dialog::init())
            .invoke_handler(tauri::generate_handler![
                commands::proxy::get_config,
                commands::proxy::save_config,
                commands::proxy::set_proxy_port,
                commands::provider::test_model_connection,
                commands::provider::resolve_effective_compression_policy,
                commands::provider::fetch_provider_catalog,
                commands::provider::fetch_provider_catalog_debug,
                commands::provider::fetch_official_models,
                commands::provider::fetch_official_models_debug,
                commands::provider::test_provider_model_connection,
                commands::activity::get_activity_log,
                commands::activity::clear_activity_log,
                commands::proxy::proxy_status,
                commands::proxy::start_proxy,
                commands::proxy::stop_proxy,
                commands::ide::discover_ide,
                commands::ide::set_custom_ide_path,
                commands::ide::reset_custom_ide_path,
                commands::ide::enable_ide_integration,
                commands::ide::launch_ide,
                commands::ide::disable_ide_integration,
                commands::util::open_path,
                commands::util::get_config_path,
                commands::util::set_native_locale,
                commands::util::open_config_dir,
                commands::util::open_external_url,
                commands::app::discover_app,
                commands::app::set_custom_app_path,
                commands::app::reset_custom_app_path,
                commands::app::enable_app_integration,
                commands::app::launch_app,
                commands::app::disable_app_integration,
                commands::cli::discover_cli,
                commands::cli::enable_cli_integration,
                commands::cli::disable_cli_integration
            ]);

    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin({
            let updater_builder = tauri_plugin_updater::Builder::new();
            let updater_builder = match option_env!("TAURI_UPDATER_PUBLIC_KEY") {
                Some(public_key) if !public_key.is_empty() => updater_builder.pubkey(public_key),
                _ => updater_builder,
            };
            updater_builder.build()
        });

    builder
        .setup(|app| {
            let locale = NativeLocale::preferred();
            let state = match create_state() {
                Ok(state) => state,
                Err(error) => {
                    tracing::error!(error = %error, "桌面状态初始化失败");
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                    let app_handle = app.handle().clone();
                    app.dialog()
                        .message(error.user_message(locale))
                        .title(locale.startup_error_title())
                        .kind(MessageDialogKind::Error)
                        .show(move |_| app_handle.exit(1));
                    return Ok(());
                }
            };
            #[cfg(target_os = "macos")]
            {
                let endpoint =
                    state::local_proxy_endpoint(state.config_store.get_config().proxy_port);
                if let Err(error) = host_integration::macos_environment::reconcile(
                    &state.host_integration_root,
                    &endpoint,
                ) {
                    tracing::warn!(%error, "恢复 macOS 宿主接入状态失败");
                }
            }
            app.manage(state);
            #[cfg(desktop)]
            {
                setup_tray(app, locale)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| tracing::error!(%error, "AGY BYOK 运行时异常退出"));
}
