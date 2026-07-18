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
use secure_storage::{delete_api_key, get_api_key, save_api_key};
use knowledge_base::commands::{KbState, init_knowledge_base};
use workspace::commands::{
    WorkspaceState, PendingProposals, PendingSleepRequests, PendingRoundsRequests, PendingQuestions, PendingToolApprovals,
    WakeRateState, init_workspace_tables,
};
use workspace::meeting::MeetingsState;
use scheduler::init_scheduler_tables;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::path::PathBuf;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::sync::Mutex;

// 关闭窗口时是否最小化到托盘（而非直接退出进程）。
// 由前端设置页通过 set_close_to_tray 命令同步，默认最小化到托盘。
struct CloseToTrayState(Arc<AtomicBool>);

// 从托盘唤起主窗口的全局快捷键，默认 Ctrl+Alt+Space，可在设置页修改。
const DEFAULT_SHOW_HOTKEY: &str = "Ctrl+Alt+Space";
struct ShowHotkeyState(StdMutex<String>);

// 进程实际在写的日志文件路径——只在 init_logging() 里算一次。
// 不能让 copy_log_file 用"现在的日期"重新拼文件名：
// 应用跨午夜运行时，文件名里的日期永远是启动那一刻的日期，"现在的日期"对不上，
// 会导致命令在文件明明存在的情况下报"日志文件不存在"
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
        // 单实例插件：必须最先注册。重复启动时不新开进程，而是唤醒已运行窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // 注册 Shell 插件 (用于打开外部链接)
        .plugin(tauri_plugin_shell::init())
        // 注册对话框插件 (用于文件选择)
        .plugin(tauri_plugin_dialog::init())
        // 注册自动更新插件 (启动时检测 GitHub Releases 上的新版本)
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 注册进程插件 (更新安装完成后重启应用)
        .plugin(tauri_plugin_process::init())
        // 注册全局快捷键插件：用于从托盘唤起主窗口
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(),
        )
        // 关闭窗口时按设置决定：最小化到托盘 或 真正退出
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window
                    .state::<CloseToTrayState>()
                    .0
                    .load(Ordering::Relaxed);
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        // 注册命令处理器
        .invoke_handler(tauri::generate_handler![
            // LLM 相关命令
            commands::llm::stream_message,
            commands::llm::cancel_stream,
            // 检测最新版本(设置页手动检测按钮)
            commands::app_update::check_latest_releases,
            // 检测并安装 Beta 版更新(独立于稳定版 updater 端点)
            commands::app_update::check_and_install_beta_update,
            // 数据库相关命令
            save_session_cmd,
            save_message_cmd,
            get_sessions_cmd,
            delete_session_cmd,
            clear_database_cmd,
            // 安全存储相关命令
            save_api_key,
            get_api_key,
            delete_api_key,
            // 知识库相关命令
            knowledge_base::commands::create_knowledge_base,
            knowledge_base::commands::list_knowledge_bases,
            knowledge_base::commands::delete_knowledge_base,
            knowledge_base::commands::import_document,
            knowledge_base::commands::list_documents,
            knowledge_base::commands::delete_document,
            knowledge_base::commands::search_knowledge_base,
            knowledge_base::commands::read_document_for_context,
            // MCP 相关命令
            commands::mcp::create_mcp_server,
            commands::mcp::list_mcp_servers,
            commands::mcp::delete_mcp_server,
            commands::mcp::get_mcp_tools,
            commands::mcp::get_all_mcp_tools,
            commands::mcp::call_mcp_tool,
            commands::mcp::test_mcp_connection,
            // 本地模型相关命令
            commands::local_model::list_local_models,
            commands::local_model::pull_local_model,
            commands::local_model::delete_local_model,
            commands::local_model::get_model_sources_cmd,
            commands::local_model::get_ollama_version,
            // Ollama 安装与服务管理
            commands::local_model::detect_ollama_installation,
            commands::local_model::start_ollama_service,
            commands::local_model::stop_ollama_service,
            commands::local_model::get_ollama_service_status,
            commands::local_model::download_ollama,
            commands::local_model::install_ollama,
            commands::local_model::search_ollama_models,
            commands::local_model::get_ollama_download_mirrors_cmd,
            // LM Studio 相关命令
            commands::lmstudio::check_lmstudio_status,
            commands::lmstudio::list_lmstudio_models,
            commands::lmstudio::pull_lmstudio_model,
            commands::lmstudio::load_lmstudio_model,
            commands::lmstudio::unload_lmstudio_model,
            // Docker 相关命令
            commands::docker::check_docker_status,
            commands::docker::list_docker_images,
            commands::docker::list_docker_containers,
            commands::docker::pull_docker_image,
            commands::docker::start_docker_container,
            commands::docker::stop_docker_container,
            commands::docker::remove_docker_container,
            commands::docker::get_docker_profiles_cmd,
            // Skill 相关命令
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
            workspace::commands::workspace_update_agent,
            workspace::commands::workspace_delete_agent,
            workspace::commands::workspace_pause_agent,
            workspace::commands::workspace_resume_agent,
            workspace::commands::workspace_send_user_message,
            workspace::commands::workspace_list_messages,
            workspace::commands::workspace_list_logs,
            workspace::commands::workspace_resolve_proposal,
            workspace::commands::workspace_resolve_sleep_request,
            workspace::commands::workspace_resolve_rounds_request,
            workspace::commands::workspace_resolve_question,
            workspace::commands::workspace_resolve_tool_approval,
            workspace::commands::workspace_list_pending_events,
            workspace::commands::workspace_list_agent_tasks,
            workspace::commands::workspace_set_task_done,
            workspace::commands::workspace_set_scratchpad,
            // 定时任务命令
            scheduler::commands::schedule_create,
            scheduler::commands::schedule_list,
            scheduler::commands::schedule_delete,
            scheduler::commands::schedule_toggle,
            // 日志相关命令
            copy_log_file,
            // 系统托盘相关命令
            set_close_to_tray,
            set_show_hotkey,
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
            // 与 workspace::db::open_conn 保持一致的 busy_timeout 设置 -- 这个连接
            // 除了做初始化，后面也被 Agent 循环自动恢复扫描复用，同样可能撞上并发写。
            if let Err(e) = conn.busy_timeout(std::time::Duration::from_secs(5)) {
                log::warn!("设置数据库 busy_timeout 失败: {}", e);
            }

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
            // Agent 循环只存在于内存里，之前重启应用后永远拿不回来，用户只能
            // 删了重建。这里把每个工作组里所有存活（未软删除）的 Agent 重新
            // 挂回一个新的后台循环——Running/WaitingApproval/WaitingAnswer/
            // Meeting 这几个状态绑定的 oneshot/会议协调器都是旧进程里的东西，
            // 已经无法恢复，重置为 Idle；Sleeping 是稳定的持久状态，不用动，
            // 新消息来了新循环自然会唤醒它。
            let workspace_state = WorkspaceState::default();
            {
                use workspace::db as ws_db;
                use workspace::types::AgentStatus;

                // 上一次运行遗留的未决事项（提议/休眠/提问/工具审批）对应的等待
                // 通道已随旧进程消亡，永远不可能再被批准或拒绝——统一标记过期，
                // 否则前端每次选中工作组都会把它们拉出来，变成一批点了就报错、
                // 又永远不消失的僵尸卡片。给对应工作组各写一条时间线日志，让
                // 用户知道这些事项去哪了。
                match ws_db::expire_unresolved_pending_events(&conn) {
                    Ok(expired) => {
                        for (workspace_id, count) in expired {
                            log::info!("应用启动：工作组 {} 有 {} 件重启前的待处理事项已过期", workspace_id, count);
                            let entry = workspace::types::WorkspaceLogEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                workspace_id,
                                agent_id: None,
                                kind: "pending_expired".to_string(),
                                content: format!(
                                    "应用重启，重启前遗留的 {} 件待处理事项已自动过期失效；如仍需要，请让对应 Agent 重新发起",
                                    count
                                ),
                                created_at: chrono::Utc::now().timestamp_millis(),
                            };
                            if let Err(e) = ws_db::insert_log(&conn, &entry) {
                                log::error!("写入待处理事项过期日志失败: {}", e);
                            }
                        }
                    }
                    Err(e) => log::error!("清理重启前遗留的待处理事项失败: {}", e),
                }

                match ws_db::list_workspaces(&conn) {
                    Ok(workspaces) => {
                        let mut resumed = 0;
                        for ws in workspaces {
                            let agents = ws_db::list_agents(&conn, &ws.id).unwrap_or_default();
                            for mut agent in agents {
                                if matches!(
                                    agent.status,
                                    AgentStatus::Running
                                        | AgentStatus::WaitingApproval
                                        | AgentStatus::WaitingAnswer
                                        | AgentStatus::Meeting
                                ) {
                                    if let Err(e) = ws_db::update_agent_status(&conn, &agent.id, AgentStatus::Idle) {
                                        log::error!("恢复 Agent 循环时重置状态失败: {}", e);
                                    }
                                    agent.status = AgentStatus::Idle;
                                }
                                workspace::commands::start_agent_loop(app.handle().clone(), workspace_state.0.clone(), agent);
                                resumed += 1;
                            }
                        }
                        log::info!("应用启动：已恢复 {} 个 Agent 的后台循环", resumed);
                    }
                    Err(e) => log::error!("恢复 Agent 循环失败（列出工作组）: {}", e),
                }

                // 补处理停机前积压的消息：恢复的循环只是重新挂上了，不会自己去
                // 翻旧账——上次关掉应用时还没来得及回复的消息会一直没人理，
                // 直到未来某条不相关的消息碰巧再唤醒它。这里全员 notify 一次：
                // 虚假唤醒去重让没有积压的 Agent 以零成本（不发起任何模型调用）
                // 跳过，休眠/暂停中的 Agent 状态也不会被这次探测动到。
                {
                    let handles = workspace_state.0.lock().unwrap();
                    for handle in handles.values() {
                        handle.notify.notify_one();
                    }
                }
            }
            app.manage(workspace_state);
            app.manage(PendingProposals::default());
            app.manage(PendingSleepRequests::default());
            app.manage(PendingRoundsRequests::default());
            app.manage(PendingQuestions::default());
            app.manage(PendingToolApprovals::default());
            app.manage(WakeRateState::default());
            app.manage(MeetingsState::default());
            app.manage(CloseToTrayState(Arc::new(AtomicBool::new(true))));
            log::info!("Database and vector store initialized");

            // 注册默认的托盘唤起快捷键（前端设置页会在启动时同步实际保存的值）
            if let Err(e) = app.global_shortcut().register(DEFAULT_SHOW_HOTKEY) {
                log::warn!("Failed to register default show-hotkey: {}", e);
            }
            app.manage(ShowHotkeyState(StdMutex::new(DEFAULT_SHOW_HOTKEY.to_string())));

            // 系统托盘图标：左键点击/菜单“显示主界面”唤回窗口，菜单“退出程序”真正结束进程
            {
                let show_item = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

                let mut tray_builder = TrayIconBuilder::new()
                    .tooltip("BaiyuAISpace")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray_builder = tray_builder.icon(icon.clone());
                }
                tray_builder.build(app)?;
            }

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

// 数据库命令的包装函数
#[tauri::command]
async fn save_session_cmd(
    session: ChatSession,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.save_session(&session).map_err(|e| commands::local_model::friendly_err("保存会话失败，请重试", e))
}

#[tauri::command]
async fn save_message_cmd(
    session_id: String,
    message: ChatMessage,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.save_message(&session_id, &message).map_err(|e| commands::local_model::friendly_err("保存消息失败，请重试", e))
}

#[tauri::command]
async fn get_sessions_cmd(
    db_state: tauri::State<'_, DbState>,
) -> Result<Vec<ChatSession>, String> {
    let db = db_state.0.lock().await;
    db.get_sessions().map_err(|e| commands::local_model::friendly_err("读取会话列表失败，请重试", e))
}

#[tauri::command]
async fn delete_session_cmd(
    session_id: String,
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.delete_session(&session_id).map_err(|e| commands::local_model::friendly_err("删除会话失败，请重试", e))
}

/// 清空数据库：删除全部会话、消息、MCP 服务器配置、Skill（设置页“危险操作”按钮对应的后端命令）
#[tauri::command]
async fn clear_database_cmd(
    db_state: tauri::State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    db.clear_all().map_err(|e| commands::local_model::friendly_err("清空数据库失败，请重启应用后重试", e))
}

#[tauri::command]
fn set_close_to_tray(enabled: bool, state: tauri::State<CloseToTrayState>) {
    state.0.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn set_show_hotkey(
    accelerator: String,
    app: tauri::AppHandle,
    state: tauri::State<ShowHotkeyState>,
) -> Result<(), String> {
    let mut current = state.0.lock().map_err(|e| commands::local_model::friendly_err("内部状态异常，请重启应用", e))?;
    if *current == accelerator {
        return Ok(());
    }
    // 先注册新快捷键，成功后再解绑旧的——避免注册失败时把原有快捷键也丢了
    app.global_shortcut()
        .register(accelerator.as_str())
        .map_err(|e| commands::local_model::friendly_err("注册快捷键失败，可能已被其他程序占用", e))?;
    let _ = app.global_shortcut().unregister(current.as_str());
    *current = accelerator;
    Ok(())
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
