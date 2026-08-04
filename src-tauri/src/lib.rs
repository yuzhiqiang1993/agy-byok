mod commands;
mod host;
mod state;

use state::create_state;
pub use state::DesktopState;

#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

#[cfg(desktop)]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show AGY BYOK", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    TrayIconBuilder::new()
        .icon(tauri::include_image!("./icons/tray-icon.png"))
        .icon_as_template(true)
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
                show_main_window(&tray.app_handle());
            }
        })
        .build(app)?;

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
    let state = create_state().expect("failed to initialize AGY BYOK desktop state");
    let builder = tauri::Builder::default();

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main_window(app);
    }));

    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::proxy::get_config,
            commands::proxy::save_config,
            commands::proxy::set_proxy_port,
            commands::provider::test_model_connection,
            commands::provider::fetch_provider_catalog,
            commands::provider::test_provider_model_connection,
            commands::activity::get_activity_log,
            commands::activity::clear_activity_log,
            commands::proxy::proxy_status,
            commands::proxy::start_proxy,
            commands::proxy::stop_proxy,
            commands::ide::discover_ide,
            commands::ide::enable_ide_integration,
            commands::ide::launch_ide,
            commands::ide::disable_ide_integration,
            commands::util::open_path,
            commands::util::open_config_dir,
            commands::util::open_external_url,
            commands::app::discover_app,
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
            #[cfg(desktop)]
            setup_tray(app)?;
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
        .expect("error while running AGY BYOK");
}
