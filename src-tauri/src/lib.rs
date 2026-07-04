mod commands;
mod db;
mod importers;
mod models;

use rusqlite::Connection;
use std::sync::Mutex;
use tauri::Manager;

/// Shared application state: a single SQLite connection behind a mutex.
pub struct AppState {
    pub db: Mutex<Connection>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Store the database in the platform-specific app data directory.
            let dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&dir).expect("failed to create app data dir");
            let db_path = dir.join("fire.db");
            let conn = db::open(&db_path).expect("failed to open database");
            app.manage(AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::create_account,
            commands::rename_account,
            commands::delete_account,
            commands::add_subaccount,
            commands::list_categories,
            commands::create_category,
            commands::delete_category,
            commands::list_transactions,
            commands::create_transaction,
            commands::update_transaction,
            commands::set_transaction_category,
            commands::set_transaction_verified,
            commands::delete_transaction,
            commands::list_rules,
            commands::delete_rule,
            commands::get_summary,
            commands::import_csv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
