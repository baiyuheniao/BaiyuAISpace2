// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * BaiyuAISpace 后端入口文件
 * 
 * 功能说明:
 * - Tauri 应用初始化和配置
 * - 数据库初始化 (SQLite)
 * - 向量数据库初始化
 * - 注册所有 Tauri 命令处理器
 * - 全局状态管理 (DbState, KbState)
 * 
 * 模块依赖:
 * - commands: LLM、认证、MCP 相关命令
 * - db: SQLite 数据库操作
 * - knowledge_base: 知识库和向量检索
 * - secure_storage: API 密钥安全存储
 */

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 引入模块
mod commands;
mod db;
mod knowledge_base;
mod secure_storage;

// 引入类型和函数
use commands::llm::{ChatMessage, ChatSession};
use db::{Database, DbState};
use secure_storage::{save_api_key, get_api_key, delete_api_key, has_api_key};
use knowledge_base::commands::{KbState, init_knowledge_base};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

/**
 * 应用入口函数
 * 
 * 初始化流程:
 * 1. 初始化日志系统
 * 2. 创建 Tokio 异步运行时
 * 3. 配置 Tauri Builder
 * 4. 注册所有命令处理器
 * 5. 初始化数据库和向量存储
 * 6. 启动应用
 */
fn main() {
    // 初始化日志系统
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // 创建 Tokio 异步运行时
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    // 构建 Tauri 应用
    tauri::Builder::default()
        // 注册 Shell 插件 (用于打开外部链接)
        .plugin(tauri_plugin_shell::init())
        // 注册对话框插件 (用于文件选择)
        .plugin(tauri_plugin_dialog::init())
        // 注册命令处理器
        .invoke_handler(tauri::generate_handler![
            // LLM 相关命令
            commands::llm::stream_message,
            commands::llm::cancel_stream,
            // Auth commands
            commands::auth::get_baidu_access_token,
            // 数据库相关命令
            save_session_cmd,
            save_message_cmd,
            get_sessions_cmd,
            delete_session_cmd,
            // 安全存储相关命令
            save_api_key,
            get_api_key,
            delete_api_key,
            has_api_key,
            // 知识库相关命令
            knowledge_base::commands::create_knowledge_base,
            knowledge_base::commands::list_knowledge_bases,
            knowledge_base::commands::delete_knowledge_base,
            knowledge_base::commands::import_document,
            knowledge_base::commands::list_documents,
            knowledge_base::commands::delete_document,
            knowledge_base::commands::search_knowledge_base,
            knowledge_base::commands::get_embedding_models,
            // MCP 相关命令
            commands::mcp::create_mcp_server,
            commands::mcp::list_mcp_servers,
            commands::mcp::delete_mcp_server,
            commands::mcp::get_mcp_tools,
            commands::mcp::get_all_mcp_tools,
            commands::mcp::call_mcp_tool,
            commands::mcp::test_mcp_connection,
        ])
        // 应用初始化设置
        .setup(move |app| {
            // 初始化 SQLite 数据库
            let db = Database::new(app.handle());
            runtime.block_on(async {
                if let Err(e) = db.init().await {
                    log::error!("Failed to initialize database: {}", e);
                }
            });
            
            let conn = match rusqlite::Connection::open(&db.path) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("Failed to open database: {}", e);
                    return Err(Box::new(e) as Box<dyn std::error::Error>);
                }
            };
            
            if let Err(e) = init_knowledge_base(&conn) {
                log::error!("Failed to initialize knowledge base tables: {}", e);
            }
            
            let app_data_dir = match app.handle().path().app_data_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    log::error!("Failed to get app data dir: {}", e);
                    return Err(Box::new(e) as Box<dyn std::error::Error>);
                }
            };
            let vector_db_path = app_data_dir.join("vector_store").to_str().unwrap_or("vector_store").to_string();
            
            let vector_store = runtime.block_on(async {
                match knowledge_base::db::VectorStore::new(&vector_db_path).await {
                    Ok(vs) => Ok(vs),
                    Err(e) => {
                        log::error!("Failed to initialize vector store: {}", e);
                        Err(e)
                    }
                }
            });
            
            let vector_store = match vector_store {
                Ok(vs) => vs,
                Err(e) => {
                    return Err(Box::new(e) as Box<dyn std::error::Error>);
                }
            };
            
            let db_path = db.path.clone();
            
            // 注册全局状态
            app.manage(DbState(Arc::new(Mutex::new(db))));
            app.manage(KbState { 
                vector_store: Arc::new(vector_store),
                db_path,
            });
            log::info!("Database and vector store initialized");
            
            Ok(())
        })
        // 运行应用
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
