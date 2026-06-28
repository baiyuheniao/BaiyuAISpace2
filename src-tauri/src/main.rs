// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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

// 引入模块
mod commands;
mod db;
mod knowledge_base;
mod scheduler;
mod secure_storage;
mod types;
mod workspace;
mod workspace_smoke_test;

// 引入类型和函数
use commands::llm::{ChatMessage, ChatSession};
use db::{Database, DbState};
use secure_storage::{save_api_key, get_api_key, delete_api_key, has_api_key};
use knowledge_base::commands::{KbState, init_knowledge_base};
use workspace::commands::{WorkspaceState, PendingProposals, PendingSleepRequests, PendingQuestions, PendingMeetingTurns, init_workspace_tables};
use scheduler::init_scheduler_tables;
use std::sync::Arc;
use std::path::PathBuf;
use tauri::Manager;
use tokio::sync::Mutex;

// 进程实际在写的日志文件路径——只在 init_logging() 里算一次。
// 不能让 get_log_path/read_log_file/copy_log_file 各自用"现在的日期"重新拼文件名：
// 应用跨午夜运行时，文件名里的日期永远是启动那一刻的日期，"现在的日期"对不上，
// 会导致这几个命令在文件明明存在的情况下报"日志文件不存在"
static LOG_FILE: once_cell::sync::OnceCell<PathBuf> = once_cell::sync::OnceCell::new();

pub fn get_log_file_path() -> Option<&'static PathBuf> {
    LOG_FILE.get()
}

fn init_logging() {
    use fern::colors::{ColoredLevelConfig, Color};
    
    // 获取应用数据目录用于存放日志
    let log_dir = if let Ok(app_data) = std::env::var("APPDATA") {
        let dir = PathBuf::from(app_data).join("BaiyuAISpace2").join("logs");
        std::fs::create_dir_all(&dir).ok();
        dir
    } else {
        PathBuf::from("logs")
    };
    
    // 创建日志文件名（按日期）
    let log_file = log_dir.join(format!(
        "app_{}.log",
        chrono::Local::now().format("%Y-%m-%d")
    ));
    let _ = LOG_FILE.set(log_file.clone());

    // 配置颜色
    let colors = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Blue)
        .debug(Color::White);
    
    // 配置日志系统
    let mut dispatch = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{}] {} - {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                colors.color(record.level()),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout());
    
    // 添加文件日志
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        dispatch = dispatch.chain(file);
    }
    
    dispatch.apply().ok();
    
    log::info!("Logging initialized, log file: {:?}", log_file);
}

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
    // 初始化日志系统 - 输出到控制台和文件
    init_logging();
    
    log::info!("Starting BaiyuAISpace2 application...");

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
            // Local model commands
            commands::local_model::check_ollama_status,
            commands::local_model::list_local_models,
            commands::local_model::show_local_model,
            commands::local_model::pull_local_model,
            commands::local_model::delete_local_model,
            commands::local_model::get_model_sources_cmd,
            commands::local_model::get_ollama_version,
            // Ollama installation & service management
            commands::local_model::detect_ollama_installation,
            commands::local_model::start_ollama_service,
            commands::local_model::stop_ollama_service,
            commands::local_model::get_ollama_service_status,
            commands::local_model::download_ollama,
            commands::local_model::install_ollama,
            commands::local_model::search_ollama_models,
            commands::local_model::get_ollama_download_mirrors_cmd,
            // LM Studio commands
            commands::lmstudio::check_lmstudio_status,
            commands::lmstudio::list_lmstudio_models,
            commands::lmstudio::pull_lmstudio_model,
            commands::lmstudio::load_lmstudio_model,
            commands::lmstudio::unload_lmstudio_model,
            // Docker commands
            commands::docker::check_docker_status,
            commands::docker::list_docker_images,
            commands::docker::list_docker_containers,
            commands::docker::pull_docker_image,
            commands::docker::start_docker_container,
            commands::docker::stop_docker_container,
            commands::docker::remove_docker_container,
            commands::docker::get_docker_profiles_cmd,
            // Skill commands
            commands::skills::save_skill,
            commands::skills::list_skills,
            commands::skills::delete_skill,
            commands::skills::add_skill_resource_file,
            commands::skills::remove_skill_resource_file,
            commands::skills::read_skill_resource_file,
            // Agent Team Mode (Workspace) 相关命令
            workspace::commands::workspace_create,
            workspace::commands::workspace_list,
            workspace::commands::workspace_delete,
            workspace::commands::workspace_create_agent_manual,
            workspace::commands::workspace_list_agents,
            workspace::commands::workspace_delete_agent,
            workspace::commands::workspace_send_user_message,
            workspace::commands::workspace_list_messages,
            workspace::commands::workspace_list_logs,
            workspace::commands::workspace_resolve_proposal,
            workspace::commands::workspace_resolve_sleep_request,
            workspace::commands::workspace_resolve_question,
            // 定时任务命令
            scheduler::commands::schedule_create,
            scheduler::commands::schedule_list,
            scheduler::commands::schedule_delete,
            scheduler::commands::schedule_toggle,
            // 日志相关命令
            get_log_path,
            read_log_file,
            copy_log_file,
        ])
        // 应用初始化设置
        .setup(move |app| {
            let db = Database::new(app.handle());
            if let Err(e) = db.init() {
                log::error!("Failed to initialize database: {}", e);
            }
            
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

            if let Err(e) = init_workspace_tables(&conn) {
                log::error!("Failed to initialize workspace tables: {}", e);
            }

            if let Err(e) = init_scheduler_tables(&conn) {
                log::error!("Failed to initialize scheduler tables: {}", e);
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
            app.manage(WorkspaceState::default());
            app.manage(PendingProposals::default());
            app.manage(PendingSleepRequests::default());
            app.manage(PendingQuestions::default());
            app.manage(PendingMeetingTurns::default());
            log::info!("Database and vector store initialized");

            // 启动定时任务调度循环
            {
                let scheduler_handle = app.handle().clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                tauri::async_runtime::spawn(async move {
                    scheduler::commands::run_scheduler_loop(scheduler_handle, cancel).await;
                });
            }

            if std::env::var("BAIYU_WORKSPACE_SMOKE_TEST").is_ok() {
                let smoke_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    workspace_smoke_test::run(smoke_handle).await;
                    log::info!("[smoke_test] 进程即将退出");
                    std::process::exit(0);
                });
            }
            
            // 确保主窗口显示并聚焦（解决窗口启动后被遮挡的问题）
            if let Some(window) = app.get_webview_window("main") {
                log::info!("Showing and focusing main window...");
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                log::info!("Main window shown and focused");
            }
            
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

#[tauri::command]
fn get_log_path() -> Result<String, String> {
    if let Some(log_file) = get_log_file_path() {
        if log_file.exists() {
            return Ok(log_file.to_string_lossy().to_string());
        }
        Err("日志文件不存在".to_string())
    } else {
        Err("无法获取日志目录".to_string())
    }
}

#[tauri::command]
fn read_log_file() -> Result<String, String> {
    if let Some(log_file) = get_log_file_path() {
        if log_file.exists() {
            return std::fs::read_to_string(log_file)
                .map_err(|e| format!("读取日志文件失败: {}", e));
        }
        Err("日志文件不存在".to_string())
    } else {
        Err("无法获取日志目录".to_string())
    }
}

#[tauri::command]
fn copy_log_file(dest_path: String) -> Result<String, String> {
    if let Some(log_file) = get_log_file_path() {
        if log_file.exists() {
            std::fs::copy(log_file, &dest_path)
                .map_err(|e| format!("复制日志文件失败: {}", e))?;
            return Ok(dest_path);
        }
        Err("日志文件不存在".to_string())
    } else {
        Err("无法获取日志目录".to_string())
    }
}
