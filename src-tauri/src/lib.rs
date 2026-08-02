mod commands;
mod host;
mod state;

pub use state::DesktopState;
use state::create_state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    let state = create_state().expect("failed to initialize AGY BYOK desktop state");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::proxy::get_config,
            commands::proxy::save_config,
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
            commands::app::discover_app,
            commands::app::enable_app_integration,
            commands::app::launch_app,
            commands::app::disable_app_integration,
            commands::cli::discover_cli,
            commands::cli::enable_cli_integration,
            commands::cli::disable_cli_integration
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGY BYOK");
}
