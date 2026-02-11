// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod secure_storage;

use commands::llm::{ChatMessage, ChatSession};
use db::{Database, DbState};
use secure_storage::{save_api_key, get_api_key, delete_api_key, has_api_key};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

fn main() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            // LLM commands
            commands::llm::stream_message,
            // Database commands
            save_session_cmd,
            save_message_cmd,
            get_sessions_cmd,
            delete_session_cmd,
            // Secure storage commands
            save_api_key,
            get_api_key,
            delete_api_key,
            has_api_key,
        ])
        .setup(|app| {
            // Initialize database
            let db = Database::new(app.handle());
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                if let Err(e) = db.init().await {
                    log::error!("Failed to initialize database: {}", e);
                }
            });
            
            app.manage(DbState(Arc::new(Mutex::new(db))));
            log::info!("Database initialized and managed");
            
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Database command wrappers
#[tauri::command]
async fn save_session_cmd(
    session: ChatSession,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.save_session(&session).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_message_cmd(
    session_id: String,
    message: ChatMessage,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.save_message(&session_id, &message).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_sessions_cmd(
    db_state: tauri::State<'_, DbState>,
) -> Result<Vec<ChatSession>, String> {
    let db = db_state.0.lock().await;
    db.get_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_session_cmd(
    session_id: String,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.delete_session(&session_id).map_err(|e| e.to_string())
}
