pub mod commands;
pub mod state;

use tauri::Manager;

use state::{load_settings, AppState};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");

            // Ensure fastembed uses an absolute cache dir so the bundled
            // app works regardless of CWD.
            if std::env::var("FASTEMBED_CACHE_DIR").is_err() {
                let cache_dir = app_data_dir.join("fastembed_cache");
                std::fs::create_dir_all(&cache_dir).ok();
                std::env::set_var("FASTEMBED_CACHE_DIR", &cache_dir);
            }

            let settings = load_settings(&app_data_dir);
            state::apply_env_keys(&settings);
            app.manage(AppState::new(settings));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::workspace_create,
            commands::workspace::workspace_open,
            commands::workspace::workspace_close,
            commands::workspace::workspace_get_info,
            commands::workspace::workspace_list_recent,
            commands::workspace::workspace_get_config,
            commands::workspace::workspace_update_config,
            commands::workspace::workspace_export_config,
            commands::search::search,
            commands::document::document_get,
            commands::document::document_list,
            commands::document::document_chunks,
            commands::document::document_import,
            commands::connector::connector_list,
            commands::connector::connector_add,
            commands::connector::connector_update,
            commands::connector::connector_remove,
            commands::connector::connector_test,
            commands::sync::sync_start,
            commands::sync::sync_cancel,
            commands::embedding::embedding_status,
            commands::embedding::embedding_run_pending,
            commands::embedding::embedding_rebuild,
            commands::embedding::embedding_update_config,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::agent::agent_list,
            commands::agent::agent_get,
            commands::agent::agent_test,
            commands::registry::registry_status,
            commands::registry::registry_list_extensions,
            commands::registry::registry_search,
            commands::registry::registry_init,
            commands::registry::registry_install,
            commands::registry::registry_update,
            commands::registry::registry_add_extension,
            commands::server::server_start,
            commands::server::server_stop,
            commands::server::server_status,
        ])
        .run(tauri::generate_context!())
        .expect("Error running Context Harness");
}
