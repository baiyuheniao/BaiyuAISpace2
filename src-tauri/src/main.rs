// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod knowledge_base;
mod secure_storage;

use commands::llm::{ChatMessage, ChatSession};
use db::{Database, DbState};
use secure_storage::{save_api_key, get_api_key, delete_api_key, has_api_key};
use knowledge_base::commands::{KbState, init_knowledge_base};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

fn main() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
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
            // Knowledge base commands
            knowledge_base::commands::create_knowledge_base,
            knowledge_base::commands::list_knowledge_bases,
            knowledge_base::commands::delete_knowledge_base,
            knowledge_base::commands::import_document,
            knowledge_base::commands::list_documents,
            knowledge_base::commands::delete_document,
            knowledge_base::commands::search_knowledge_base,
            knowledge_base::commands::get_embedding_models,
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
            
            // Initialize knowledge base tables
            let conn = rusqlite::Connection::open(&db.path).expect("Failed to open DB");
            if let Err(e) = init_knowledge_base(&conn) {
                log::error!("Failed to initialize knowledge base tables: {}", e);
            }
            
            // Initialize vector store
            let app_data_dir = app.handle().path().app_data_dir().expect("Failed to get app data dir");
            let vector_db_path = app_data_dir.join("vector_store").to_str().unwrap().to_string();
            
            let vector_store = runtime.block_on(async {
                knowledge_base::db::VectorStore::new(&vector_db_path).await
                    .expect("Failed to initialize vector store")
            });
            
            // Clone path before moving db
            let db_path = db.path.clone();
            
            app.manage(DbState(Arc::new(Mutex::new(db))));
            app.manage(KbState { 
                vector_store: Arc::new(vector_store),
                db_path,
            });
            log::info!("Database and vector store initialized");
            
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
