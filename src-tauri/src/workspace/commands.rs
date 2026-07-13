// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::db;
use super::meeting::{self, MeetingCheckIn, MeetingConfig, MeetingHandle, MeetingsState};
use super::types::*;
use crate::commands::llm::{
    append_text_reply, append_tool_round, build_native_messages, build_skill_context, run_turn,
    ChatMessage, PendingToolCall, TurnOutcome,
};
use crate::commands::mcp::{call_mcp_tool, get_all_mcp_tools, MCPTool};
use crate::db::DbState;
use crate::knowledge_base::commands::{search_knowledge_base, KbState};
use crate::knowledge_base::retrieval::build_context as build_rag_context;
use crate::knowledge_base::types::{RetrievalMode, RetrievalRequest};
use crate::secure_storage;
use chrono::Utc;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const PROPOSAL_TIMEOUT_SECS: u64 = 600;
const MAX_ROUNDS_PER_WAKE: u32 = 8;
/// 会议轮次（屏障式签到）不计入 `MAX_ROUNDS_PER_WAKE`，否则长会议会被正常
/// 工具轮上限拦腰截断；这个总轮数是防失控的兜底保险丝。
const MAX_TOTAL_ROUNDS_PER_WAKE: u32 = 500;
const MAX_HISTORY_MESSAGES: i64 = 40;
/// 唤醒频率护栏：滑动窗口内唤醒次数超过这个数就自动暂停这个 Agent，防止
/// Agent 间来回搭话失控、无节制消耗 API 调用（多 Agent 自主互聊没有刹车的
/// 那个风险）。正常使用（人工来回对话、一场会议）远远达不到这个频率。
const WAKE_RATE_WINDOW_SECS: i64 = 300;
const MAX_WAKES_PER_WINDOW: usize = 20;

/// 一个运行中 Agent 的唤醒信号 + 停止开关。只存在于内存里，但 `main.rs` 的
/// `setup()` 在启动时会对每个活跃 Agent 再调一次 `start_agent_loop`（走的是
/// 同一个函数），所以应用重启后各工作组的 Agent 会恢复运行，而不是永久沉睡。
pub struct AgentHandle {
    pub notify: Arc<Notify>,
    pub cancel: CancellationToken,
}

/// 运行中 Agent 循环的注册表，key 是 agent id（id 是 UUID，天然全局唯一，
/// 所以跨工作组用一个扁平 map 就够了）。
#[derive(Default)]
pub struct WorkspaceState(pub Arc<Mutex<HashMap<String, AgentHandle>>>);

/// 用户对主 Agent 的 `workspace_create_agent` 提议做出的决定。`Approved`
/// 携带的是*最终*的 request，由前端确认卡片填入（它会补上 `api_config_id`/
/// `base_url`，这些是模型不可能知道的）。
pub enum ProposalDecision {
    Approved(Box<CreateAgentRequest>),
    Rejected,
}

#[derive(Default)]
pub struct PendingProposals(pub Arc<Mutex<HashMap<String, oneshot::Sender<ProposalDecision>>>>);

/// 一个子 Agent 待处理的 `workspace_sleep` 请求，key 是生成的 request id。
/// 由主 Agent 调用 `workspace_approve_sleep`/`workspace_reject_sleep` 解决，
/// 或者由用户通过 `workspace_resolve_sleep_request` 直接越权处理——谁先到谁
/// 生效，因为把这条记录从 map 里摘掉这个动作本身就等于拿到了处理权。
#[derive(Default)]
pub struct PendingSleepRequests(pub Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>);

/// 一个待处理的 `workspace_asks` 问题，key 是生成的 question id。由用户通过
/// `workspace_resolve_question` 回答来解决。
#[derive(Default)]
pub struct PendingQuestions(pub Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>);

/// 一个待处理的 MCP 工具调用审批，key 是生成的 approval id。只有在发起调用
/// 的 Agent `require_tool_approval` 为 true 时才会触发——见 `dispatch_tool_call`
/// 的兜底分支。
#[derive(Default)]
pub struct PendingToolApprovals(pub Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>);

/// 每个 Agent 最近的唤醒时间戳（毫秒），是个固定大小的滑动窗口——用来防止
/// 失控互相搭话：如果一个 Agent 在 `WAKE_RATE_WINDOW_SECS` 内被唤醒次数超过
/// `MAX_WAKES_PER_WINDOW`，就自动暂停它。刻意只存内存不落库，因为重启会
/// 自然把计数清零，这没问题——这个护栏只需要在单次运行会话内抓住失控行为
/// 就够了。
#[derive(Default)]
pub struct WakeRateState(pub Arc<Mutex<HashMap<String, VecDeque<i64>>>>);

pub fn init_workspace_tables(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    db::init_workspace_tables(conn)
}

// ---------------------------------------------------------------------------
// Tauri command（前端可直接调用的命令）
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_create(
    request: CreateWorkspaceRequest,
    db_state: State<'_, DbState>,
) -> Result<Workspace, WorkspaceError> {
    if request.name.trim().is_empty() {
        return Err(WorkspaceError::InvalidConfig("工作组名称不能为空".to_string()));
    }
    let now = Utc::now().timestamp_millis();
    let workspace = Workspace {
        id: Uuid::new_v4().to_string(),
        name: request.name,
        description: request.description,
        max_agents: request.max_agents.unwrap_or(DEFAULT_MAX_AGENTS).max(1),
        created_at: now,
        updated_at: now,
    };
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::insert_workspace(&conn, &workspace)?;
    Ok(workspace)
}

#[tauri::command]
pub async fn workspace_list(db_state: State<'_, DbState>) -> Result<Vec<Workspace>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_workspaces(&conn)
}

#[tauri::command]
pub async fn workspace_delete(
    workspace_id: String,
    db_state: State<'_, DbState>,
    workspace_state: State<'_, WorkspaceState>,
    meetings: State<'_, MeetingsState>,
) -> Result<(), WorkspaceError> {
    let agent_ids: Vec<String> = {
        let db = db_state.0.lock().await;
        let conn = db::open_conn(&db.path)?;
        db::list_agents(&conn, &workspace_id)?.into_iter().map(|a| a.id).collect()
    };

    {
        let mut handles = workspace_state.0.lock().unwrap();
        for id in &agent_ids {
            if let Some(handle) = handles.remove(id) {
                handle.cancel.cancel();
            }
        }
    }

    // 注销进行中的会议：丢掉注册表里的 sender，阻止新的签到进来；协调器
    // 会在集合超时后因所有人缺席而自行收场。
    {
        let mut map = meetings.0.lock().unwrap();
        map.remove(&workspace_id);
    }

    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::delete_workspace(&conn, &workspace_id)?;
    Ok(())
}

/// 手动创建路径：用户填表单，这里直接生成 Agent 及其后台循环。另一条创建
/// 路径——主 Agent 的 `workspace_create_agent` 工具——在用户通过
/// `workspace_resolve_proposal` 批准提议之后，也会汇入同一个
/// `spawn_agent_internal`，所以两条路径从这里往后就完全一致了。
#[tauri::command]
pub async fn workspace_create_agent_manual(
    request: CreateAgentRequest,
    app_handle: AppHandle,
    workspace_state: State<'_, WorkspaceState>,
) -> Result<WorkspaceAgent, WorkspaceError> {
    spawn_agent_internal(&app_handle, workspace_state.0.clone(), request).await
}

/// 返回包括软删除在内的全部 Agent——前端也需要已删除的那些，才能在历史
/// 消息/日志里正确解析出发送者名字，而不是显示一串原始 UUID；前端自己会
/// 用 `deletedAt` 把它们从活跃名册/下拉框里过滤掉。
#[tauri::command]
pub async fn workspace_list_agents(
    workspace_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceAgent>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_agents_including_deleted(&conn, &workspace_id)
}

/// 把用户的编辑应用到一个已存在的 Agent（名字/模型/提示词/工具权限/RAG 配置）。
/// 刻意不重启 Agent 的后台循环——`process_agent_wake` 每次唤醒开始时都会
/// 从数据库重新读一遍这一行，所以普通配置更新在它下一轮就会生效。
#[tauri::command]
pub async fn workspace_update_agent(
    request: UpdateAgentRequest,
    db_state: State<'_, DbState>,
) -> Result<WorkspaceAgent, WorkspaceError> {
    if request.name.trim().is_empty() || request.provider.trim().is_empty() || request.model.trim().is_empty() {
        return Err(WorkspaceError::InvalidConfig("name/provider/model 不能为空".to_string()));
    }
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::update_agent(&conn, &request)?;
    db::get_agent(&conn, &request.id)?.ok_or_else(|| WorkspaceError::AgentNotFound(request.id.clone()))
}

#[tauri::command]
pub async fn workspace_delete_agent(
    agent_id: String,
    db_state: State<'_, DbState>,
    workspace_state: State<'_, WorkspaceState>,
) -> Result<(), WorkspaceError> {
    {
        let mut handles = workspace_state.0.lock().unwrap();
        if let Some(handle) = handles.remove(&agent_id) {
            handle.cancel.cancel();
        }
    }
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::delete_agent(&conn, &agent_id)?;
    Ok(())
}

/// 手动紧急停止：立刻把 Agent 标记为 Paused，此后它的循环每次唤醒都会
/// 空操作跳过，直到被恢复。不会取消当前这一轮已经在跑的工作（那一轮会
/// 正常跑完）；只是阻止*下一轮*开始。
#[tauri::command]
pub async fn workspace_pause_agent(
    agent_id: String,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    let agent = load_agent(&app_handle, &agent_id).await?.ok_or_else(|| WorkspaceError::AgentNotFound(agent_id.clone()))?;
    set_agent_status(&app_handle, &agent_id, AgentStatus::Paused).await;
    insert_workspace_log(&app_handle, &agent.workspace_id, Some(agent_id.clone()), "paused", format!("「{}」已手动暂停", agent.name)).await;
    Ok(())
}

#[tauri::command]
pub async fn workspace_resume_agent(
    agent_id: String,
    app_handle: AppHandle,
    workspace_state: State<'_, WorkspaceState>,
) -> Result<(), WorkspaceError> {
    let agent = load_agent(&app_handle, &agent_id).await?.ok_or_else(|| WorkspaceError::AgentNotFound(agent_id.clone()))?;
    set_agent_status(&app_handle, &agent_id, AgentStatus::Idle).await;
    insert_workspace_log(&app_handle, &agent.workspace_id, Some(agent_id.clone()), "resumed", format!("「{}」已恢复运行", agent.name)).await;
    // 暂停期间到达的消息，其唤醒已经消耗掉了 `Notify` 的 permit（然后在暂停
    // 检查那里被空操作跳过了）——如果这里不显式再 notify 一次，积压的消息
    // 就会一直没人回应，直到某条不相关的未来消息碰巧再把循环唤醒。
    if let Some(handle) = workspace_state.0.lock().unwrap().get(&agent_id) {
        handle.notify.notify_one();
    }
    Ok(())
}

/// 用户往工作组里发一条消息——发给某个具体 Agent，或者用
/// `to_agent_id: "all"` 广播给所有人。一个刚创建的 Agent 也是靠这个拿到
/// 第一次唤醒：Agent 的循环起步时是沉睡的（挂在自己的 `Notify` 上），只有
/// 真的有消息发给它才会开始跑，所以一个全新的 Agent 永远不会在零上下文
/// 的情况下被要求回复。
#[tauri::command]
pub async fn workspace_send_user_message(
    workspace_id: String,
    to_agent_id: String,
    content: String,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    if content.trim().is_empty() {
        return Err(WorkspaceError::InvalidConfig("消息内容不能为空".to_string()));
    }
    send_workspace_message(&app_handle, &workspace_id, "user", &to_agent_id, &content).await;

    // Agent 后台循环只存在于内存中（见 `start_agent_loop`），应用重新启动后
    // 不会自动恢复。如果目标 Agent 的 handle 不存在，上面 `send_workspace_message`
    // 内部的 notify 就静默地什么也没做：消息本身照常存库、照常显示，但不会
    // 有人真正去处理它。这里告诉前端一声，让它能提醒用户，而不是让用户干等
    // 一个永远不会来的回复。
    let has_live_handle = {
        let workspace_state = app_handle.state::<WorkspaceState>();
        let handles = workspace_state.0.lock().unwrap();
        handles.contains_key(&to_agent_id)
    };
    if !has_live_handle {
        if let Ok(Some(agent)) = load_agent(&app_handle, &to_agent_id).await {
            let _ = app_handle.emit(
                "workspace://agent-inactive",
                serde_json::json!({ "agentId": agent.id, "agentName": agent.name }),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn workspace_list_messages(
    workspace_id: String,
    limit: Option<i64>,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceMessage>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_messages(&conn, &workspace_id, limit.unwrap_or(500))
}

#[tauri::command]
pub async fn workspace_list_logs(
    workspace_id: String,
    limit: Option<i64>,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceLogEntry>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_logs(&conn, &workspace_id, limit.unwrap_or(500))
}

/// 前端响应 `workspace://agent-proposal` 事件调用此命令：用户查看/编辑完
/// 主 Agent 提议的子 Agent 配置后，点了批准或拒绝。`approved` 为 true 时，
/// `request` 必须是完整的、经用户确认的 `CreateAgentRequest`（包括模型从
/// 未提供过的 `api_config_id`/`base_url`）。
#[tauri::command]
pub async fn workspace_resolve_proposal(
    proposal_id: String,
    approved: bool,
    request: Option<CreateAgentRequest>,
    pending: State<'_, PendingProposals>,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&proposal_id)
    };
    let Some(sender) = sender else {
        // 发起这条等待的阻塞调用已经不在了（超时收场、被别人抢先处理，或者
        // 应用重启过）。顺手把数据库里的记录也标掉并广播移除，免得它变成一张
        // 永远点不动的僵尸卡片。
        mark_pending_event_resolved(&app_handle, &proposal_id).await;
        return Err(WorkspaceError::NotFound(format!("提议 {} 已过期或已被处理", proposal_id)));
    };

    let decision = if approved {
        match request {
            Some(req) => ProposalDecision::Approved(Box::new(req)),
            None => {
                return Err(WorkspaceError::InvalidConfig(
                    "同意创建时必须提供完整的 Agent 配置".to_string(),
                ))
            }
        }
    } else {
        ProposalDecision::Rejected
    };

    let _ = sender.send(decision);
    mark_pending_event_resolved(&app_handle, &proposal_id).await;
    Ok(())
}

/// 让用户绕过主 Agent，直接批准/拒绝一个子 Agent 待处理的 `workspace_sleep`
/// 请求——对应设计文档里"用户也能直接代为批准/拒绝"这条越权规则。谁先把
/// 这条记录从 map 里摘掉（这里，或者主 Agent 调用 `workspace_approve_sleep`/
/// `workspace_reject_sleep`），谁的决定就真正生效。
#[tauri::command]
pub async fn workspace_resolve_sleep_request(
    request_id: String,
    approved: bool,
    pending: State<'_, PendingSleepRequests>,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&request_id)
    };
    let Some(sender) = sender else {
        mark_pending_event_resolved(&app_handle, &request_id).await;
        return Err(WorkspaceError::NotFound(format!("休眠请求 {} 已过期或已被处理", request_id)));
    };
    let _ = sender.send(approved);
    mark_pending_event_resolved(&app_handle, &request_id).await;
    Ok(())
}

/// 前端在用户对着 `workspace://question` 事件弹出的卡片输入完答案后调用。
#[tauri::command]
pub async fn workspace_resolve_question(
    question_id: String,
    answer: String,
    pending: State<'_, PendingQuestions>,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&question_id)
    };
    let Some(sender) = sender else {
        mark_pending_event_resolved(&app_handle, &question_id).await;
        return Err(WorkspaceError::NotFound(format!("问题 {} 已过期或已被处理", question_id)));
    };
    let _ = sender.send(answer);
    mark_pending_event_resolved(&app_handle, &question_id).await;
    Ok(())
}

/// 列出这个工作组里所有还在等人工决策的事项——前端在选中一个工作组时会拉
/// 一次，这样即便页面（或整个应用）没打开期间提出了提议/休眠请求/问题，
/// 它们也不会像与之配对的一次性事件那样直接丢失。
#[tauri::command]
pub async fn workspace_list_pending_events(
    workspace_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspacePendingEvent>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_unresolved_pending_events(&conn, &workspace_id)
}

/// 前端在用户批准/拒绝一张 `workspace://tool-approval` 卡片后调用。
#[tauri::command]
pub async fn workspace_resolve_tool_approval(
    approval_id: String,
    approved: bool,
    pending: State<'_, PendingToolApprovals>,
    app_handle: AppHandle,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&approval_id)
    };
    let Some(sender) = sender else {
        mark_pending_event_resolved(&app_handle, &approval_id).await;
        return Err(WorkspaceError::NotFound(format!("工具调用审批 {} 已过期或已被处理", approval_id)));
    };
    let _ = sender.send(approved);
    mark_pending_event_resolved(&app_handle, &approval_id).await;
    Ok(())
}

/// 让前端能直接查看/管理一个 Agent 的结构化待办清单，而不是只能让 Agent
/// 自己通过 `workspace_task_list` 工具去管理。
#[tauri::command]
pub async fn workspace_list_agent_tasks(
    agent_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceAgentTask>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::list_tasks(&conn, &agent_id)
}

#[tauri::command]
pub async fn workspace_set_task_done(
    task_id: String,
    done: bool,
    db_state: State<'_, DbState>,
) -> Result<(), WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::set_task_done(&conn, &task_id, done)
}

/// 让用户能直接查看/编辑/清空一个 Agent 的工作备忘（scratchpad），而不是只能
/// 任由 Agent 自己通过 workspace_scratchpad 工具维护——Agent 记了什么、记错了
/// 什么，用户得看得见也改得动，这是"透明可控"的底线。传空字符串即清空。
#[tauri::command]
pub async fn workspace_set_scratchpad(
    agent_id: String,
    content: String,
    db_state: State<'_, DbState>,
) -> Result<(), WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::set_scratchpad(&conn, &agent_id, &content)
}

/// 标记一条待处理事项已解决，并广播 `workspace://pending-resolved` 事件让
/// 前端移除对应卡片——不管它是被用户点的、被主 Agent 用工具处理的、还是
/// 超时/取消自动收场的。没有这个事件，凡是"不经前端的手"解决的事项，卡片
/// 都会在界面上一直挂着，用户再点一次就报"不存在或已被处理"。
async fn mark_pending_event_resolved(app_handle: &AppHandle, id: &str) {
    {
        let db_state = app_handle.state::<DbState>();
        let db = db_state.0.lock().await;
        match db::open_conn(&db.path) {
            Ok(conn) => {
                if let Err(e) = db::resolve_pending_event(&conn, id) {
                    log::error!("[workspace] 标记待处理事项已解决失败: {}", e);
                }
            }
            Err(e) => log::error!("[workspace] 打开数据库连接失败（标记待处理事项）: {}", e),
        }
    }
    let _ = app_handle.emit("workspace://pending-resolved", serde_json::json!({ "id": id }));
}

async fn record_pending_event(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent_id: &str,
    agent_name: &str,
    kind: &str,
    payload: serde_json::Value,
    id: &str,
) {
    let event = WorkspacePendingEvent {
        id: id.to_string(),
        workspace_id: workspace_id.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent_name.to_string(),
        kind: kind.to_string(),
        payload,
        created_at: Utc::now().timestamp_millis(),
        resolved_at: None,
    };
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    match db::open_conn(&db.path) {
        Ok(conn) => {
            if let Err(e) = db::insert_pending_event(&conn, &event) {
                log::error!("[workspace] 持久化待处理事项失败: {}", e);
            }
        }
        Err(e) => log::error!("[workspace] 打开数据库连接失败（持久化待处理事项）: {}", e),
    }
}

// ---------------------------------------------------------------------------
// 两条创建路径和 Agent 循环共用的内部辅助函数
// ---------------------------------------------------------------------------

pub(crate) async fn load_agent(app_handle: &AppHandle, agent_id: &str) -> Result<Option<WorkspaceAgent>, WorkspaceError> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::get_agent(&conn, agent_id)
}

async fn load_workspace(app_handle: &AppHandle, workspace_id: &str) -> Result<Option<Workspace>, WorkspaceError> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    let conn = db::open_conn(&db.path)?;
    db::get_workspace(&conn, workspace_id)
}

pub(crate) async fn set_agent_status(app_handle: &AppHandle, agent_id: &str, status: AgentStatus) {
    let db_state = app_handle.state::<DbState>();
    {
        let db = db_state.0.lock().await;
        match db::open_conn(&db.path) {
            Ok(conn) => {
                if let Err(e) = db::update_agent_status(&conn, agent_id, status) {
                    log::error!("[workspace] 更新 Agent 状态失败: {}", e);
                }
            }
            Err(e) => log::error!("[workspace] 打开数据库连接失败（更新状态）: {}", e),
        }
    }
    let _ = app_handle.emit(
        "workspace://agent-status",
        serde_json::json!({ "agentId": agent_id, "status": status.as_str() }),
    );
}

pub async fn insert_workspace_log(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent_id: Option<String>,
    kind: &str,
    content: String,
) {
    let entry = WorkspaceLogEntry {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        agent_id,
        kind: kind.to_string(),
        content,
        created_at: Utc::now().timestamp_millis(),
    };
    let db_state = app_handle.state::<DbState>();
    {
        let db = db_state.0.lock().await;
        match db::open_conn(&db.path) {
            Ok(conn) => {
                if let Err(e) = db::insert_log(&conn, &entry) {
                    log::error!("[workspace] 写入活动日志失败: {}", e);
                }
            }
            Err(e) => log::error!("[workspace] 打开数据库连接失败（写日志）: {}", e),
        }
    }
    let _ = app_handle.emit("workspace://log", &entry);
}

/// 落库一条消息，并唤醒它指向的那个（或那些）Agent。`to_agent_id` 为
/// `"all"` 时，只唤醒**这个工作组**的成员——启动时全量恢复循环之后，注册表
/// 里挂着所有工作组的 Agent，按注册表广播会把别的工作组吵醒：多数情况下
/// 虚假唤醒去重能兜住，但一个历史停在未回复消息上的 Agent 会凭空开始推理，
/// 休眠中的 Agent 也会被无谓打扰。
pub async fn send_workspace_message(
    app_handle: &AppHandle,
    workspace_id: &str,
    from_agent_id: &str,
    to_agent_id: &str,
    content: &str,
) {
    send_workspace_message_impl(app_handle, workspace_id, from_agent_id, to_agent_id, content, true).await
}

/// 静默变体：照常落库 + 发前端事件，但不唤醒任何 Agent。会议发言用它存档——
/// 与会者正挂在会议签到上，发言已经通过工具结果送达它们了，再 notify 只会
/// 在散会后多触发一轮毫无新内容的唤醒。
pub async fn send_workspace_message_silent(
    app_handle: &AppHandle,
    workspace_id: &str,
    from_agent_id: &str,
    to_agent_id: &str,
    content: &str,
) {
    send_workspace_message_impl(app_handle, workspace_id, from_agent_id, to_agent_id, content, false).await
}

async fn send_workspace_message_impl(
    app_handle: &AppHandle,
    workspace_id: &str,
    from_agent_id: &str,
    to_agent_id: &str,
    content: &str,
    wake: bool,
) {
    log::info!(
        "[workspace] 消息路由: {} → {} | {}...",
        from_agent_id,
        to_agent_id,
        content.chars().take(80).collect::<String>()
    );
    let msg = WorkspaceMessage {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        from_agent_id: from_agent_id.to_string(),
        to_agent_id: to_agent_id.to_string(),
        content: content.to_string(),
        created_at: Utc::now().timestamp_millis(),
    };

    let db_state = app_handle.state::<DbState>();
    {
        let db = db_state.0.lock().await;
        match db::open_conn(&db.path) {
            Ok(conn) => {
                if let Err(e) = db::insert_message(&conn, &msg) {
                    log::error!("[workspace] 写入消息失败: {}", e);
                }
            }
            Err(e) => log::error!("[workspace] 打开数据库连接失败（写消息）: {}", e),
        }
    }
    let _ = app_handle.emit("workspace://message", &msg);

    if !wake {
        return;
    }
    if to_agent_id == "all" {
        // 先查成员名单再锁 handle 表——std::sync::Mutex 的锁不能跨 await 持有。
        let member_ids: Vec<String> = list_agents_for_workspace(app_handle, workspace_id)
            .await
            .into_iter()
            .map(|a| a.id)
            .collect();
        let workspace_state = app_handle.state::<WorkspaceState>();
        let handles = workspace_state.0.lock().unwrap();
        for id in &member_ids {
            if id != from_agent_id {
                if let Some(handle) = handles.get(id) {
                    handle.notify.notify_one();
                }
            }
        }
    } else {
        let workspace_state = app_handle.state::<WorkspaceState>();
        let handles = workspace_state.0.lock().unwrap();
        if let Some(handle) = handles.get(to_agent_id) {
            handle.notify.notify_one();
        }
    }
}

async fn list_agents_for_workspace(app_handle: &AppHandle, workspace_id: &str) -> Vec<WorkspaceAgent> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    match db::open_conn(&db.path) {
        Ok(conn) => db::list_agents(&conn, workspace_id).unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn find_main_agent_id(app_handle: &AppHandle, workspace_id: &str) -> Option<String> {
    list_agents_for_workspace(app_handle, workspace_id)
        .await
        .into_iter()
        .find(|a| a.role == AgentRole::Main)
        .map(|a| a.id)
}

/// 一个子 Agent 的休眠请求被批准（或有子 Agent 进入 Error 状态）之后，检查
/// 这个工作组里是不是所有子 Agent 都已停止推进；如果是，给主 Agent 发消息
/// （同时也会唤醒它），请它验收一下任务是否已经完成。
///
/// Error 也算"不会再自己推进"的终态——只认 Sleeping 的话，一个报错卡死的
/// 子 Agent 会让验收永远无法触发，任务无声烂尾。但要求至少有一个真的在
/// 休眠：全员报错的场景由逐个的出错上报（`notify_main_agent_of_error`）
/// 覆盖，再发一条"请验收"只会误导主 Agent 以为有成果可验。
async fn maybe_trigger_main_agent_review(app_handle: &AppHandle, workspace_id: &str) {
    let agents = list_agents_for_workspace(app_handle, workspace_id).await;
    let subs: Vec<_> = agents.iter().filter(|a| a.role == AgentRole::Sub).collect();
    if subs.is_empty() {
        return;
    }
    let sleeping = subs.iter().filter(|a| a.status == AgentStatus::Sleeping).count();
    let errored = subs.iter().filter(|a| a.status == AgentStatus::Error).count();
    if sleeping == 0 || sleeping + errored < subs.len() {
        return;
    }
    let Some(main) = agents.iter().find(|a| a.role == AgentRole::Main) else {
        return;
    };

    let status_brief = if errored > 0 {
        format!("（休眠 {} 个，异常 {} 个）", sleeping, errored)
    } else {
        String::new()
    };
    send_workspace_message(
        app_handle,
        workspace_id,
        "system",
        &main.id,
        &format!(
            "工作组内所有子 Agent 都已停止推进{}，请验收当前任务进度：如果任务已经完成，用 workspace_message \
             告知用户；如果还没完成，可以用 workspace_message 叫醒某个子 Agent 继续推进（给异常状态的 Agent \
             发消息也会让它重试），或者用 workspace_create_agent 创建新的 Agent。",
            status_brief
        ),
    )
    .await;
    insert_workspace_log(
        app_handle,
        workspace_id,
        None,
        "acceptance_review",
        format!("所有子 Agent 已停止推进{}，已唤醒主 Agent 验收任务进度", status_brief),
    )
    .await;
}

/// 子 Agent 进入 Error 状态时，把出错的事实和原因告知主 Agent（同时唤醒它），
/// 让它决定重试、换人还是上报用户——否则一个悄悄倒下的 Agent 只会留下一条
/// 日志，任务无声烂尾。主 Agent 自己出错时没有更上级可通知，状态标签和
/// 错误日志的悬浮提示已经能让用户看到。
async fn notify_main_agent_of_error(app_handle: &AppHandle, workspace_id: &str, agent_id: &str, error: &str) {
    let agents = list_agents_for_workspace(app_handle, workspace_id).await;
    let Some(errored) = agents.iter().find(|a| a.id == agent_id) else {
        return;
    };
    if errored.role == AgentRole::Main {
        return;
    }
    let Some(main) = agents.iter().find(|a| a.role == AgentRole::Main) else {
        return;
    };
    send_workspace_message(
        app_handle,
        workspace_id,
        "system",
        &main.id,
        &format!(
            "Agent「{}」处理消息时出错：{}。请决定下一步：给它发消息让它重试、用 workspace_message 告知用户\
             检查它的配置、或安排其他 Agent 接手它的工作。",
            errored.name, error
        ),
    )
    .await;
}

/// 手动创建命令和一个被批准的 `workspace_create_agent` 提议共用这个函数：
/// 校验 Agent 数量安全上限、插入数据库行、启动 Agent 的后台循环。
async fn spawn_agent_internal(
    app_handle: &AppHandle,
    agent_handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
    request: CreateAgentRequest,
) -> Result<WorkspaceAgent, WorkspaceError> {
    if request.name.trim().is_empty() || request.provider.trim().is_empty() || request.model.trim().is_empty() {
        return Err(WorkspaceError::InvalidConfig("name/provider/model 不能为空".to_string()));
    }

    let workspace = load_workspace(app_handle, &request.workspace_id)
        .await?
        .ok_or_else(|| WorkspaceError::NotFound(request.workspace_id.clone()))?;

    let db_state = app_handle.state::<DbState>();
    let now = Utc::now().timestamp_millis();
    let agent = {
        let db = db_state.0.lock().await;
        let conn = db::open_conn(&db.path)?;

        let current_count = db::count_agents(&conn, &workspace.id)?;
        if current_count >= workspace.max_agents as i64 {
            return Err(WorkspaceError::AgentLimitReached(format!(
                "工作组「{}」Agent 数量已达上限 ({})，不能再创建新的 Agent",
                workspace.name, workspace.max_agents
            )));
        }

        let agent = WorkspaceAgent {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace.id.clone(),
            name: request.name.clone(),
            role: request.role,
            provider: request.provider.clone(),
            model: request.model.clone(),
            base_url: request.base_url.clone(),
            api_config_id: request.api_config_id.clone(),
            system_prompt: request.system_prompt.clone(),
            mcp_server_ids: request.mcp_server_ids.clone(),
            knowledge_base_ids: request.knowledge_base_ids.clone(),
            active_skill_ids: request.active_skill_ids.clone(),
            status: AgentStatus::Idle,
            rag_top_k: request.rag_top_k,
            rag_retrieval_mode: request.rag_retrieval_mode.clone(),
            rag_reranker_config_id: request.rag_reranker_config_id.clone(),
            rag_reranker_base_url: request.rag_reranker_base_url.clone(),
            rag_reranker_model: request.rag_reranker_model.clone(),
            rag_rerank_top_n: request.rag_rerank_top_n,
            scratchpad: String::new(),
            require_tool_approval: request.require_tool_approval,
            enable_thinking: request.enable_thinking,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        db::insert_agent(&conn, &agent)?;
        agent
    };

    insert_workspace_log(
        app_handle,
        &workspace.id,
        Some(agent.id.clone()),
        "agent_created",
        format!(
            "已创建 Agent「{}」（{} / {}，角色：{}）",
            agent.name,
            agent.provider,
            agent.model,
            agent.role.as_str()
        ),
    )
    .await;

    start_agent_loop(app_handle.clone(), agent_handles, agent.clone());

    Ok(agent)
}

/// 刻意写成同步函数（不是 `async fn`）：它做的事只是往 `std::sync::Mutex`
/// 里做一次不阻塞的快速插入，然后把循环任务丢出去。保持同步是为了打破一个
/// 间接的递归 async 环，否则会把 rustc 的 Send 检查绕晕——当一个主 Agent 的
/// 提议被批准时，`run_agent_loop` 会经由 `process_agent_wake` ->
/// `dispatch_tool_call` -> `propose_agent_creation` -> `spawn_agent_internal`
/// 这条链路又绕回这里，如果这里也是 `async fn`，这条链就会自己引用自己。
pub(crate) fn start_agent_loop(app_handle: AppHandle, agent_handles: Arc<Mutex<HashMap<String, AgentHandle>>>, agent: WorkspaceAgent) {
    let notify = Arc::new(Notify::new());
    let cancel = CancellationToken::new();

    {
        let mut handles = agent_handles.lock().unwrap();
        handles.insert(agent.id.clone(), AgentHandle { notify: notify.clone(), cancel: cancel.clone() });
    }

    let workspace_id = agent.workspace_id.clone();
    let agent_id = agent.id.clone();

    tauri::async_runtime::spawn(async move {
        run_agent_loop(app_handle, workspace_id, agent_id, notify, cancel).await;
    });
}

/// Agent 的常驻后台任务：挂在 `notify` 上睡眠，直到有东西找上它，处理完
/// 这期间积累的一切之后再重新睡回去。一直运行到 `cancel` 触发为止（工作组
/// 或 Agent 被删除）。
async fn run_agent_loop(
    app_handle: AppHandle,
    workspace_id: String,
    agent_id: String,
    notify: Arc<Notify>,
    cancel: CancellationToken,
) {
    // 只在"进入 Error"的那一刻通知主 Agent 一次，连续出错不重复刷屏；
    // 成功跑完一轮就复位，之后再出错会重新通知。
    let mut error_notified = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = notify.notified() => {}
        }
        if cancel.is_cancelled() {
            break;
        }

        match process_agent_wake(&app_handle, &workspace_id, &agent_id, &cancel).await {
            Ok(()) => {
                error_notified = false;
            }
            Err(e) => {
                log::error!("Workspace agent {} 处理失败: {}", agent_id, e);
                insert_workspace_log(&app_handle, &workspace_id, Some(agent_id.clone()), "error", e.to_string()).await;
                // 用户已手动暂停的 Agent 保持 Paused，别用 Error 覆盖掉这个
                // 明确的人为决定；也不再向主 Agent 上报（用户已经介入了）。
                let paused = matches!(
                    load_agent(&app_handle, &agent_id).await,
                    Ok(Some(a)) if a.status == AgentStatus::Paused
                );
                if !paused {
                    set_agent_status(&app_handle, &agent_id, AgentStatus::Error).await;
                    if !error_notified {
                        error_notified = true;
                        notify_main_agent_of_error(&app_handle, &workspace_id, &agent_id, &e.to_string()).await;
                        // Error 属于验收条件里的终态之一：这个 Agent 一倒，
                        // 可能恰好凑齐"全员停止推进"。
                        maybe_trigger_main_agent_review(&app_handle, &workspace_id).await;
                    }
                }
            }
        }
    }
    log::info!("Workspace agent {} 循环已停止", agent_id);
}

/// 一次"唤醒"：重新加载 Agent 当前配置，回放跟它相关的消息历史，然后
/// 在模型调用和工具执行之间反复交替（受 `MAX_ROUNDS_PER_WAKE` 约束），
/// 直到模型给出一段纯文本回复而不是又一次工具调用为止。
async fn process_agent_wake(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent_id: &str,
    cancel: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let agent = load_agent(app_handle, agent_id)
        .await?
        .ok_or_else(|| WorkspaceError::AgentNotFound(agent_id.to_string()))?;
    let workspace = load_workspace(app_handle, workspace_id)
        .await?
        .ok_or_else(|| WorkspaceError::NotFound(workspace_id.to_string()))?;

    // 暂停中：用户手动暂停过，或已经被下面的唤醒频率护栏自动暂停过，静默
    // 跳过，直到用户手动 workspace_resume_agent。
    if agent.status == AgentStatus::Paused {
        log::debug!("[workspace] Agent「{}」处于暂停状态，跳过本次唤醒", agent.name);
        return Ok(());
    }

    // 唤醒频率护栏：滑动窗口内唤醒次数超阈值就自动暂停，防止 Agent 间来回
    // 搭话失控、无节制消耗 API 调用。这一次触发阈值的唤醒本身也不处理。
    {
        let rate_state = app_handle.state::<WakeRateState>();
        let now = Utc::now().timestamp_millis();
        let window_start = now - WAKE_RATE_WINDOW_SECS * 1000;
        let exceeded = {
            let mut map = rate_state.0.lock().unwrap();
            let entry = map.entry(agent_id.to_string()).or_default();
            entry.retain(|&t| t >= window_start);
            entry.push_back(now);
            entry.len() > MAX_WAKES_PER_WINDOW
        };
        if exceeded {
            log::warn!(
                "[workspace] Agent「{}」{} 秒内被唤醒超过 {} 次，自动暂停以防失控",
                agent.name, WAKE_RATE_WINDOW_SECS, MAX_WAKES_PER_WINDOW
            );
            set_agent_status(app_handle, agent_id, AgentStatus::Paused).await;
            insert_workspace_log(
                app_handle,
                workspace_id,
                Some(agent_id.to_string()),
                "auto_paused",
                format!(
                    "「{}」{} 秒内被唤醒超过 {} 次，已自动暂停（防止无节制消耗 API 调用），需要手动恢复",
                    agent.name, WAKE_RATE_WINDOW_SECS, MAX_WAKES_PER_WINDOW
                ),
            )
            .await;
            return Ok(());
        }
    }

    log::info!(
        "[workspace] 唤醒 Agent「{}」({}) - workspace: {} model: {}/{}",
        agent.name, agent_id, workspace.name, agent.provider, agent.model
    );

    // 注意：确认"确实有新内容要处理"之前不碰状态。以前这里先把状态改成
    // Running、空转检查不通过再改成 Idle——一次虚假唤醒（比如广播的余波）就
    // 会把休眠中 Agent 的 Sleeping 状态洗成 Idle，"全员休眠→触发验收"的
    // 条件随之被无声破坏。
    let chat_history = build_chat_history(app_handle, workspace_id, &agent).await;
    if chat_history.is_empty() {
        log::debug!("[workspace] Agent「{}」历史消息为空，跳过本次唤醒（状态保持 {:?}）", agent.name, agent.status);
        return Ok(());
    }
    // 虚假唤醒去重：`Notify` 的 permit 语义下，处理期间收到的通知会在处理结束
    // 后再触发一轮唤醒；如果这时最新一条相关消息就是自己上次发的回复，说明
    // 没有任何人在这之后说过话，没有新内容可回应，直接跳过，避免对着同一段
    // 历史重新推理一遍、很可能又重复回复一次。
    if chat_history.last().map(|m| m.role.as_str()) == Some("assistant") {
        log::debug!("[workspace] Agent「{}」自上次发言后没有新消息，跳过本次唤醒（状态保持 {:?}）", agent.name, agent.status);
        return Ok(());
    }

    // Agent 在轮转发言时保持 Meeting 状态可见；只有开始一次普通（非会议）
    // 唤醒时，才把状态提升为 Running。
    if agent.status != AgentStatus::Meeting {
        set_agent_status(app_handle, agent_id, AgentStatus::Running).await;
    }
    let latest_query = chat_history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let system_prompt = build_agent_system_prompt(app_handle, &agent, &latest_query).await;
    let mut native_messages = build_native_messages(&agent.provider, &chat_history);

    // 本地模型（比如 Ollama）不需要 API 密钥——跟 llm.rs 请求层
    // `get_api_key()` 里的同一条例外规则保持一致。
    let api_key = if agent.provider == "local" {
        String::new()
    } else {
        secure_storage::get_api_key(agent.api_config_id.clone())
            .map_err(|e| WorkspaceError::InvalidConfig(e.to_string()))?
            .ok_or_else(|| {
                WorkspaceError::InvalidConfig(format!(
                    "Agent「{}」找不到可用的 API 密钥（原本引用的 API 配置可能已在设置页被删除，或从未设置过密钥）；\
                     请在设置页重新配置对应的 API 密钥，或编辑这个 Agent 换一个 API 配置",
                    agent.name
                ))
            })?
    };

    // 无条件拉取：内置工具（server_id "builtin"，网络搜索/网页抓取）不在
    // agent.mcp_server_ids 白名单可选范围内（UI 里压根不会把它列成一个可勾选
    // 的服务器），但设计意图是它像聊天页一样「开箱即用、不需要额外配置」——
    // 之前这里被 mcp_server_ids 是否为空整个短路掉，导致没勾选任何外部 MCP
    // 服务器的 Agent（包括没配置过 MCP 的主 Agent）永远拿不到这两个内置工具。
    let mut tools = workspace_tool_defs(&agent);
    {
        let db_state = app_handle.state::<DbState>();
        match get_all_mcp_tools(db_state).await {
            Ok(all_tools) => {
                tools.extend(
                    all_tools
                        .into_iter()
                        .filter(|t| t.server_id == "builtin" || agent.mcp_server_ids.contains(&t.server_id)),
                );
            }
            Err(e) => log::warn!("Workspace agent {} 获取 MCP 工具列表失败: {}", agent_id, e),
        }
    }

    log::debug!(
        "[workspace] Agent「{}」开始推理 - 历史 {} 条消息，可用工具 {} 个",
        agent.name, chat_history.len(), tools.len()
    );
    let mut produced_final_text = false;
    // 会议轮次（全部工具调用都是会议签到且没出错的轮）不占用普通工具轮
    // 配额，让屏障式会议能开满自己的发言上限；MAX_TOTAL_ROUNDS_PER_WAKE 兜底。
    let mut counted_rounds: u32 = 0;
    let mut total_rounds: u32 = 0;
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if counted_rounds >= MAX_ROUNDS_PER_WAKE || total_rounds >= MAX_TOTAL_ROUNDS_PER_WAKE {
            break;
        }
        total_rounds += 1;

        log::debug!("[workspace] Agent「{}」第 {} 轮推理", agent.name, total_rounds);
        let outcome = run_turn(
            &agent.provider,
            &agent.model,
            &api_key,
            &agent.base_url,
            Some(&system_prompt),
            &native_messages,
            &tools,
            // 传 None——让各个 provider 应用自己那套宽裕的默认值（Anthropic
            // 是 32000，其余是各模型自己的上限），而不是沿用旧代码里硬编码的
            // 4096，那会悄悄截断长回复。
            None,
            agent.enable_thinking,
        )
        .await
        .map_err(|e| WorkspaceError::LlmError(e.to_string()))?;

        match outcome {
            TurnOutcome::Text(text) => {
                log::info!(
                    "[workspace] Agent「{}」产出最终回复 ({} 字符)",
                    agent.name, text.len()
                );
                append_text_reply(&agent.provider, &mut native_messages, &text);
                if !text.trim().is_empty() {
                    send_workspace_message(app_handle, workspace_id, agent_id, "user", &text).await;
                }
                produced_final_text = true;
                break;
            }
            TurnOutcome::ToolCalls(calls) => {
                log::info!(
                    "[workspace] Agent「{}」调用 {} 个工具: {}",
                    agent.name, calls.len(),
                    calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
                );
                let mut results = Vec::with_capacity(calls.len());
                for call in &calls {
                    insert_workspace_log(
                        app_handle,
                        workspace_id,
                        Some(agent_id.to_string()),
                        "tool_call",
                        format!("调用工具 {} 参数: {}", call.name, call.arguments),
                    )
                    .await;
                    let result = dispatch_tool_call(app_handle, &workspace, &agent, call, cancel).await;
                    log::debug!(
                        "[workspace] 工具 {} 返回: {}",
                        call.name,
                        result.to_string().chars().take(200).collect::<String>()
                    );
                    results.push(result);
                }
                let meeting_round = calls
                    .iter()
                    .all(|c| matches!(c.name.as_str(), "workspace_meeting" | "workspace_meeting_checkin"))
                    && results.iter().all(|r| r.get("error").is_none());
                if !meeting_round {
                    counted_rounds += 1;
                }
                append_tool_round(&agent.provider, &mut native_messages, &calls, &results);
            }
        }
    }

    if !produced_final_text {
        log::warn!(
            "Workspace agent {} 在轮数上限内没有给出最终回复，提前结束本次唤醒（计数 {} 轮 / 总 {} 轮）",
            agent_id,
            counted_rounds,
            total_rounds
        );
    }

    // 别覆盖掉 Sleeping（由 workspace_sleep 设置）、Meeting（由会议协调器
    // 管理，散会时协调器自己会还原与会者状态），也别覆盖 Paused——用户在
    // 这轮进行中按下的"暂停"是紧急停止，如果这里照旧复位成 Idle，暂停就
    // 会在当前轮跑完的瞬间自动失效，下一条消息照常把它唤醒。
    let blocking = matches!(
        load_agent(app_handle, agent_id).await,
        Ok(Some(WorkspaceAgent {
            status: AgentStatus::Sleeping | AgentStatus::Meeting | AgentStatus::Paused,
            ..
        }))
    );
    if !blocking {
        set_agent_status(app_handle, agent_id, AgentStatus::Idle).await;
    }
    Ok(())
}

/// 跟这个 Agent 相关的近期消息，转换成 `build_native_messages` 期望的扁平
/// `ChatMessage` 结构。发给它的消息会加上发送者的显示名字前缀，这样多方
/// 对话对模型来说依然可读；Agent 自己发过的历史消息则原样保留，因为它
/// 本来就知道那是自己说的话。
async fn build_chat_history(app_handle: &AppHandle, workspace_id: &str, agent: &WorkspaceAgent) -> Vec<ChatMessage> {
    let db_state = app_handle.state::<DbState>();
    let (messages, agents) = {
        let db = db_state.0.lock().await;
        let conn = match db::open_conn(&db.path) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let messages = db::list_recent_messages_for_agent(&conn, workspace_id, &agent.id, MAX_HISTORY_MESSAGES)
            .unwrap_or_default();
        // 这里也要包含软删除的 Agent——一个已删除 Agent 过去发的消息仍然
        // 落在这个历史窗口里，需要能正确解析出发送者名字，而不是给模型看
        // 一串原始 id。
        let agents = db::list_agents_including_deleted(&conn, workspace_id).unwrap_or_default();
        (messages, agents)
    };

    let name_of = |id: &str| -> String {
        if id == "user" {
            return "用户".to_string();
        }
        if id == "all" {
            return "所有人".to_string();
        }
        if id == "system" {
            return "系统".to_string();
        }
        agents.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| id.to_string())
    };

    messages
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let is_own = m.from_agent_id == agent.id;
            let content = if is_own { m.content } else { format!("[来自 {}]: {}", name_of(&m.from_agent_id), m.content) };
            ChatMessage {
                id: format!("wm_{}", i),
                role: if is_own { "assistant".to_string() } else { "user".to_string() },
                content,
                timestamp: m.created_at,
                error: None,
                images: vec![],
                videos: vec![],
            }
        })
        .collect()
}

/// 把 Agent 自己的系统提示词，跟它启用的 Skill 的说明合并；如果它配置了
/// 知识库，还会为最新收到的消息检索 RAG 上下文并合并进来——这里直接复用
/// 普通聊天模式的 `search_knowledge_base`/`build_context`，而不是在这里
/// 重新实现一套检索逻辑。
async fn build_agent_system_prompt(app_handle: &AppHandle, agent: &WorkspaceAgent, latest_query: &str) -> String {
    let mut sections = vec![agent.system_prompt.clone()];

    // 工作记忆：每次唤醒的上下文只由最近 40 条消息重建，工具调用轮次的中间
    // 结果醒来就丢——scratchpad 是这个空白之外唯一跨唤醒保留的私有存储，靠
    // workspace_scratchpad 工具自己读写维护，内容原样拼进系统提示词。
    if !agent.scratchpad.trim().is_empty() {
        sections.push(format!(
            "【你的工作备忘（可用 workspace_scratchpad 工具更新）】\n{}",
            agent.scratchpad
        ));
    }

    {
        let db_state = app_handle.state::<DbState>();
        let db = db_state.0.lock().await;
        if let Ok(conn) = db::open_conn(&db.path) {
            if let Ok(tasks) = db::list_tasks(&conn, &agent.id) {
                if !tasks.is_empty() {
                    let lines: Vec<String> = tasks
                        .iter()
                        .map(|t| format!("- [{}] {} (id: {})", if t.done { "x" } else { " " }, t.content, t.id))
                        .collect();
                    sections.push(format!(
                        "【你的任务清单（可用 workspace_task_list 工具更新）】\n{}",
                        lines.join("\n")
                    ));
                }
            }
        }
    }

    if !agent.active_skill_ids.is_empty() {
        let db_state = app_handle.state::<DbState>();
        let all_skills = {
            let db = db_state.0.lock().await;
            match db.get_skills() {
                Ok(skills) => skills,
                Err(e) => {
                    log::warn!("Workspace agent {} 读取 Skill 列表失败: {}", agent.id, e);
                    vec![]
                }
            }
        };
        let active: Vec<_> = all_skills.into_iter().filter(|s| agent.active_skill_ids.contains(&s.id)).collect();
        if !active.is_empty() {
            sections.push(build_skill_context(&active, app_handle).await);
        }
    }

    if !agent.knowledge_base_ids.is_empty() && !latest_query.is_empty() {
        let kb_state = app_handle.state::<KbState>();
        for kb_id in &agent.knowledge_base_ids {
            let request = RetrievalRequest {
                kb_id: kb_id.clone(),
                query: latest_query.to_string(),
                top_k: agent.rag_top_k,
                retrieval_mode: match agent.rag_retrieval_mode.as_str() {
                    "vector" => RetrievalMode::Vector,
                    "keyword" => RetrievalMode::Keyword,
                    _ => RetrievalMode::Hybrid,
                },
                similarity_threshold: 0.0,
                window_size: 1,
                reranker_config_id: agent.rag_reranker_config_id.clone(),
                reranker_base_url: agent.rag_reranker_base_url.clone(),
                reranker_model: agent.rag_reranker_model.clone(),
                rerank_top_n: agent.rag_rerank_top_n,
            };
            match search_knowledge_base(request, kb_state.clone()).await {
                Ok(result) if !result.chunks.is_empty() => {
                    sections.push(build_rag_context(&result.chunks, &result.query));
                }
                Ok(_) => {}
                Err(e) => log::warn!("Workspace agent {} 知识库 {} 检索失败: {}", agent.id, kb_id, e),
            }
        }
    }

    sections.join("\n\n---\n\n")
}

/// 始终可用的 Workspace 工具集。`workspace_create_agent`/
/// `workspace_approve_sleep`/`workspace_reject_sleep` 只有主 Agent 能用；
/// `workspace_sleep` 只有子 Agent 能用（主 Agent 是负责监督全局的那个，让
/// 它去休眠没有意义）；`workspace_message`/`workspace_agent_list`/
/// `workspace_asks` 所有人都能用。
fn workspace_tool_defs(agent: &WorkspaceAgent) -> Vec<MCPTool> {
    let mut tools = vec![
        MCPTool {
            server_id: "workspace".to_string(),
            server_name: "workspace".to_string(),
            name: "workspace_message".to_string(),
            description: "向工作组内的其他 Agent 或用户发送一条消息。to_agent_id 填具体 Agent 的 id，或填 \"all\" 广播给所有人。"
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to_agent_id": { "type": "string", "description": "目标 Agent 的 id，或 \"all\" 广播给所有人" },
                    "content": { "type": "string", "description": "消息内容" }
                },
                "required": ["to_agent_id", "content"]
            }),
        },
        MCPTool {
            server_id: "workspace".to_string(),
            server_name: "workspace".to_string(),
            name: "workspace_agent_list".to_string(),
            description: "查看当前工作组内已有哪些 Agent，包括它们的角色、provider/model 和当前状态。".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        MCPTool {
            server_id: "workspace".to_string(),
            server_name: "workspace".to_string(),
            name: "workspace_asks".to_string(),
            description: "向用户提一个问题并等待回答（用户会在前端看到弹出的提问卡片）。如果用户在限定时间内没有回答，会收到一个表示\"未回答\"的结果。"
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "question": { "type": "string", "description": "要问用户的问题" } },
                "required": ["question"]
            }),
        },
    ];

    tools.push(MCPTool {
        server_id: "workspace".to_string(),
        server_name: "workspace".to_string(),
        name: "workspace_scratchpad".to_string(),
        description: "读写你的私人自由文本备忘。这是跨越「唤醒」持续保留的私有存储之一（普通对话历史只保留\
            最近几十条消息，工具调用的中间结果醒来就丢）——用它记录已查到的资料、当前想法、下一步打算做什么。\
            如果是可勾选完成的具体待办事项，优先用 workspace_task_list 而不是这里。\
            action 为 replace 时整体覆盖，append 时追加一行，clear 清空；省略 content 直接读出当前内容。"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["read", "replace", "append", "clear"], "description": "read 只读取；replace 整体覆盖；append 追加一行；clear 清空" },
                "content": { "type": "string", "description": "replace/append 时要写入的内容" }
            },
            "required": ["action"]
        }),
    });
    tools.push(MCPTool {
        server_id: "workspace".to_string(),
        server_name: "workspace".to_string(),
        name: "workspace_task_list".to_string(),
        description: "管理你的结构化任务清单，跨「唤醒」持续保留。add 新增一条待办；complete 把某条标记完成；\
            reopen 取消完成标记；remove 删除一条；list 列出全部（含已完成）。清单内容也会自动拼进你的系统提示词，\
            不用每次都手动 list 来提醒自己。"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["add", "complete", "reopen", "remove", "list"], "description": "要执行的操作" },
                "content": { "type": "string", "description": "action=add 时新任务的内容" },
                "task_id": { "type": "string", "description": "action=complete/reopen/remove 时目标任务的 id" }
            },
            "required": ["action"]
        }),
    });
    tools.push(MCPTool {
        server_id: "workspace".to_string(),
        server_name: "workspace".to_string(),
        name: "workspace_log".to_string(),
        description: "向工作组的共享活动日志写入一条记录（所有 Agent 和用户都能在活动时间线里看到）。".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "content": { "type": "string", "description": "要写入日志的内容" } },
            "required": ["content"]
        }),
    });
    tools.push(MCPTool {
        server_id: "workspace".to_string(),
        server_name: "workspace".to_string(),
        name: "workspace_meeting".to_string(),
        description: "发起一次工作组会议（每个工作组同时只能有一场）。组内其他 Agent 会被邀请参会，\
            大家轮流发言；轮到某位与会者发言时，它会一次性收到此前错过的全部发言。你是本场会议的\
            主持人：想散会时，在签到（workspace_meeting_checkin）时把 end_meeting 设为 true。"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "会议议题" },
                "content": { "type": "string", "description": "你的开场发言" },
                "max_speeches": { "type": "integer", "description": "发言总数上限（可选，默认 30，最大 200），达到后主持人须总结散会" }
            },
            "required": ["topic", "content"]
        }),
    });
    tools.push(MCPTool {
        server_id: "workspace".to_string(),
        server_name: "workspace".to_string(),
        name: "workspace_meeting_checkin".to_string(),
        description: "会议签到/发言工具。收到会议邀请后立即调用它签到（content 留空）；签到后工具会\
            挂起等待，轮到你发言或会议结束时才返回结果（附上你错过的全部发言）。每次收到本工具的结果，\
            都按结果里的 instruction 再次调用：轮到你发言时把发言写进 content。\
            主持人可以在任意一次签到时把 end_meeting 设为 true 结束会议。"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "meeting_id": { "type": "string", "description": "会议 id（会议邀请里给出）" },
                "content": { "type": "string", "description": "轮到你发言时的发言内容；未轮到时留空" },
                "end_meeting": { "type": "boolean", "description": "仅主持人有效：结束会议" }
            },
            "required": ["meeting_id"]
        }),
    });

    if agent.role == AgentRole::Main {
        tools.push(MCPTool {
            server_id: "workspace".to_string(),
            server_name: "workspace".to_string(),
            name: "workspace_create_agent".to_string(),
            description: "提议创建一个新的子 Agent 来协助完成任务。这个提议会先展示给用户确认，用户同意后才会真正创建，不会自动生效。"
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "新 Agent 的名字" },
                    "provider": { "type": "string", "description": "openai / anthropic / google 等" },
                    "model": { "type": "string", "description": "模型名称" },
                    "system_prompt": { "type": "string", "description": "这个新 Agent 的职责说明/系统提示词" }
                },
                "required": ["name", "provider", "model", "system_prompt"]
            }),
        });
        for (name, verb) in [("workspace_approve_sleep", "批准"), ("workspace_reject_sleep", "拒绝")] {
            tools.push(MCPTool {
                server_id: "workspace".to_string(),
                server_name: "workspace".to_string(),
                name: name.to_string(),
                description: format!(
                    "{}一个子 Agent 的休眠申请。request_id 填该子 Agent 申请休眠时系统消息里给出的请求 id。",
                    verb
                ),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "request_id": { "type": "string", "description": "休眠请求的 id" } },
                    "required": ["request_id"]
                }),
            });
        }
    } else {
        tools.push(MCPTool {
            server_id: "workspace".to_string(),
            server_name: "workspace".to_string(),
            name: "workspace_sleep".to_string(),
            description: "在当前任务阶段没有更多事情可做时，申请进入休眠状态（默认需要主 Agent 批准，用户也可以直接代为批准/拒绝）。\
                之后如果有新消息发给你，会自动把你叫醒。"
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "reason": { "type": "string", "description": "申请休眠的原因，比如已完成的工作内容" } }
            }),
        });
    }

    tools
}

/// 执行模型发起的一次工具调用，始终返回一个 JSON 值（出错时变成
/// `{"error": ...}` 而不是往外传播异常），因为这个结果会直接原样喂回给
/// 模型，作为这次工具调用的输出。
async fn dispatch_tool_call(
    app_handle: &AppHandle,
    workspace: &Workspace,
    agent: &WorkspaceAgent,
    call: &PendingToolCall,
    cancel: &CancellationToken,
) -> serde_json::Value {
    match call.name.as_str() {
        "workspace_message" => {
            let to_agent_id = call.arguments.get("to_agent_id").and_then(|v| v.as_str()).unwrap_or("all").to_string();
            let content = call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if content.trim().is_empty() {
                return serde_json::json!({ "error": "content 不能为空" });
            }
            if to_agent_id == agent.id {
                return serde_json::json!({ "error": "不能给自己发消息，请使用 workspace_agent_list 查看其他 Agent 的 id" });
            }
            send_workspace_message(app_handle, &workspace.id, &agent.id, &to_agent_id, &content).await;
            serde_json::json!({ "status": "sent", "to": to_agent_id })
        }
        "workspace_agent_list" => {
            let db_state = app_handle.state::<DbState>();
            let agents = {
                let db = db_state.0.lock().await;
                match db::open_conn(&db.path) {
                    Ok(conn) => db::list_agents(&conn, &workspace.id).unwrap_or_default(),
                    Err(_) => vec![],
                }
            };
            let summary: Vec<_> = agents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id, "name": a.name, "role": a.role.as_str(),
                        "provider": a.provider, "model": a.model, "status": a.status.as_str(),
                        "is_self": a.id == agent.id,
                    })
                })
                .collect();
            serde_json::json!(summary)
        }
        "workspace_create_agent" => {
            if agent.role != AgentRole::Main {
                return serde_json::json!({ "error": "只有主 Agent 才能提议创建新的子 Agent" });
            }

            let current_count = {
                let db_state = app_handle.state::<DbState>();
                let db = db_state.0.lock().await;
                db::open_conn(&db.path)
                    .ok()
                    .and_then(|conn| db::count_agents(&conn, &workspace.id).ok())
                    .unwrap_or(0)
            };
            if current_count >= workspace.max_agents as i64 {
                return serde_json::json!({
                    "error": format!("工作组 Agent 数量已达上限 ({})，不能再创建新的 Agent", workspace.max_agents)
                });
            }

            let draft = CreateAgentRequest {
                workspace_id: workspace.id.clone(),
                name: call.arguments.get("name").and_then(|v| v.as_str()).unwrap_or("新 Agent").to_string(),
                role: AgentRole::Sub,
                provider: call.arguments.get("provider").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                model: call.arguments.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                base_url: String::new(),
                api_config_id: String::new(),
                system_prompt: call.arguments.get("system_prompt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                mcp_server_ids: vec![],
                knowledge_base_ids: vec![],
                active_skill_ids: vec![],
                rag_top_k: default_rag_top_k(),
                rag_retrieval_mode: default_rag_retrieval_mode(),
                rag_reranker_config_id: None,
                rag_reranker_base_url: None,
                rag_reranker_model: None,
                rag_rerank_top_n: None,
                require_tool_approval: default_require_tool_approval(),
                enable_thinking: false,
            };

            let pending = app_handle.state::<PendingProposals>();
            propose_agent_creation(app_handle, &pending, &workspace.id, agent, draft, cancel).await
        }
        "workspace_sleep" => {
            if agent.role == AgentRole::Main {
                return serde_json::json!({ "error": "主 Agent 不能申请休眠" });
            }
            let reason = call.arguments.get("reason").and_then(|v| v.as_str()).unwrap_or("未说明").to_string();

            set_agent_status(app_handle, &agent.id, AgentStatus::WaitingApproval).await;
            let approved = request_sleep_approval(app_handle, &workspace.id, agent, &reason, cancel).await;
            if approved {
                set_agent_status(app_handle, &agent.id, AgentStatus::Sleeping).await;
                maybe_trigger_main_agent_review(app_handle, &workspace.id).await;
                serde_json::json!({ "status": "approved" })
            } else {
                set_agent_status(app_handle, &agent.id, AgentStatus::Running).await;
                serde_json::json!({ "status": "rejected", "message": "休眠申请被拒绝，请继续当前任务" })
            }
        }
        "workspace_approve_sleep" | "workspace_reject_sleep" => {
            if agent.role != AgentRole::Main {
                return serde_json::json!({ "error": "只有主 Agent 才能审批休眠申请" });
            }
            let request_id = match call.arguments.get("request_id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => return serde_json::json!({ "error": "缺少 request_id" }),
            };
            let approved = call.name == "workspace_approve_sleep";
            let pending = app_handle.state::<PendingSleepRequests>();
            let sender = pending.0.lock().unwrap().remove(&request_id);
            match sender {
                Some(tx) => {
                    let _ = tx.send(approved);
                    mark_pending_event_resolved(app_handle, &request_id).await;
                    serde_json::json!({ "status": "ok" })
                }
                None => serde_json::json!({ "error": format!("休眠请求 {} 不存在或已被处理/超时", request_id) }),
            }
        }
        "workspace_asks" => {
            let question = call.arguments.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if question.trim().is_empty() {
                return serde_json::json!({ "error": "question 不能为空" });
            }
            set_agent_status(app_handle, &agent.id, AgentStatus::WaitingAnswer).await;
            let answer = request_user_answer(app_handle, &workspace.id, agent, &question, cancel).await;
            set_agent_status(app_handle, &agent.id, AgentStatus::Running).await;
            serde_json::json!({ "answer": answer })
        }
        "workspace_scratchpad" => {
            let action = call.arguments.get("action").and_then(|v| v.as_str()).unwrap_or("read");
            let db_state = app_handle.state::<DbState>();
            let new_content = match action {
                "clear" => Some(String::new()),
                "replace" => Some(call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string()),
                "append" => {
                    let line = call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    if line.is_empty() {
                        return serde_json::json!({ "error": "append 需要非空 content" });
                    }
                    let current = agent.scratchpad.clone();
                    Some(if current.is_empty() { line } else { format!("{}\n{}", current, line) })
                }
                "read" => None,
                other => return serde_json::json!({ "error": format!("未知 action: {}", other) }),
            };
            if let Some(content) = &new_content {
                let db = db_state.0.lock().await;
                match db::open_conn(&db.path) {
                    Ok(conn) => {
                        if let Err(e) = db::set_scratchpad(&conn, &agent.id, content) {
                            return serde_json::json!({ "error": format!("写入失败: {}", e) });
                        }
                    }
                    Err(e) => return serde_json::json!({ "error": format!("打开数据库连接失败: {}", e) }),
                }
            }
            let current = match &new_content {
                Some(c) => c.clone(),
                None => {
                    // 读操作现查一次库，而不是用调用这个工具时已经在内存里的
                    // agent 快照——万一同一次唤醒里工具轮之间有过其他写入
                    // （目前不会，但语义上"读"就该读最新值）。
                    let db = db_state.0.lock().await;
                    match db::open_conn(&db.path) {
                        Ok(conn) => db::get_scratchpad(&conn, &agent.id).unwrap_or_else(|_| agent.scratchpad.clone()),
                        Err(_) => agent.scratchpad.clone(),
                    }
                }
            };
            serde_json::json!({ "scratchpad": current })
        }
        "workspace_task_list" => {
            let action = call.arguments.get("action").and_then(|v| v.as_str()).unwrap_or("list");
            let db_state = app_handle.state::<DbState>();
            let db = db_state.0.lock().await;
            let conn = match db::open_conn(&db.path) {
                Ok(c) => c,
                Err(e) => return serde_json::json!({ "error": format!("打开数据库连接失败: {}", e) }),
            };
            match action {
                "add" => {
                    let content = call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                    if content.is_empty() {
                        return serde_json::json!({ "error": "add 需要非空 content" });
                    }
                    let now = Utc::now().timestamp_millis();
                    let task = WorkspaceAgentTask { id: Uuid::new_v4().to_string(), agent_id: agent.id.clone(), content, done: false, created_at: now, updated_at: now };
                    if let Err(e) = db::insert_task(&conn, &task) {
                        return serde_json::json!({ "error": format!("新增任务失败: {}", e) });
                    }
                    // 告诉前端这个 Agent 的任务清单变了——否则用户面板上看到的
                    // 一直是旧清单，只能靠手动刷新按钮兜底。
                    let _ = app_handle.emit("workspace://tasks-updated", serde_json::json!({ "agentId": agent.id }));
                    serde_json::json!({ "status": "added", "taskId": task.id })
                }
                "complete" | "reopen" => {
                    let Some(task_id) = call.arguments.get("task_id").and_then(|v| v.as_str()) else {
                        return serde_json::json!({ "error": "缺少 task_id" });
                    };
                    if let Err(e) = db::set_task_done(&conn, task_id, action == "complete") {
                        return serde_json::json!({ "error": e.to_string() });
                    }
                    let _ = app_handle.emit("workspace://tasks-updated", serde_json::json!({ "agentId": agent.id }));
                    serde_json::json!({ "status": "ok" })
                }
                "remove" => {
                    let Some(task_id) = call.arguments.get("task_id").and_then(|v| v.as_str()) else {
                        return serde_json::json!({ "error": "缺少 task_id" });
                    };
                    if let Err(e) = db::delete_task(&conn, task_id) {
                        return serde_json::json!({ "error": format!("删除任务失败: {}", e) });
                    }
                    let _ = app_handle.emit("workspace://tasks-updated", serde_json::json!({ "agentId": agent.id }));
                    serde_json::json!({ "status": "removed" })
                }
                "list" => match db::list_tasks(&conn, &agent.id) {
                    Ok(tasks) => serde_json::json!(tasks),
                    Err(e) => serde_json::json!({ "error": format!("读取任务清单失败: {}", e) }),
                },
                other => serde_json::json!({ "error": format!("未知 action: {}", other) }),
            }
        }
        "workspace_log" => {
            let content = call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if content.trim().is_empty() {
                return serde_json::json!({ "error": "content 不能为空" });
            }
            insert_workspace_log(app_handle, &workspace.id, Some(agent.id.clone()), "agent_note", content).await;
            serde_json::json!({ "status": "logged" })
        }
        "workspace_meeting" => {
            let topic = call.arguments.get("topic").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if topic.is_empty() {
                return serde_json::json!({ "error": "topic 不能为空" });
            }
            let opening = call.arguments.get("content").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if opening.is_empty() {
                return serde_json::json!({ "error": "content（开场发言）不能为空" });
            }
            let max_speeches = call
                .arguments
                .get("max_speeches")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(meeting::DEFAULT_MAX_SPEECHES)
                .clamp(1, meeting::MAX_MAX_SPEECHES);

            let all_agents = list_agents_for_workspace(app_handle, &workspace.id).await;
            // 暂停中的 Agent 不拉进会议 -- 暂停是用户明确要求它别再干活，会议邀请
            // 不应该把它绕过去。
            let others: Vec<_> = all_agents.iter().filter(|a| a.id != agent.id && a.status != AgentStatus::Paused).collect();
            if others.is_empty() {
                return serde_json::json!({ "error": "工作组内没有其他可参会的 Agent（可能都已暂停），无法召开会议" });
            }

            let meeting_id = Uuid::new_v4().to_string();
            let (tx, event_rx) = mpsc::channel::<MeetingCheckIn>(64);
            {
                let meetings = app_handle.state::<MeetingsState>();
                let mut map = meetings.0.lock().unwrap();
                if map.contains_key(&workspace.id) {
                    return serde_json::json!({ "error": "本工作组已有一场会议正在进行，请等它结束后再发起" });
                }
                map.insert(workspace.id.clone(), MeetingHandle { meeting_id: meeting_id.clone(), tx: tx.clone() });
            }

            // 发言顺序：主持人在前，其余按创建顺序（list_agents 已按 created_at 排序）。
            let participants: Vec<(String, String)> = std::iter::once((agent.id.clone(), agent.name.clone()))
                .chain(others.iter().map(|a| (a.id.clone(), a.name.clone())))
                .collect();
            let order_display = participants.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" → ");
            // 记住入场前正在休眠的与会者：散会时还原成休眠而不是一律复位待命。
            // 开会把人叫起来没问题（会议邀请也是消息），但不能顺手吞掉它们
            // "任务已完成"的声明，否则"全员休眠→触发验收"的机制会被开会打断。
            let sleeping_before: std::collections::HashSet<String> = others
                .iter()
                .filter(|a| a.status == AgentStatus::Sleeping)
                .map(|a| a.id.clone())
                .collect();

            for (id, _) in &participants {
                set_agent_status(app_handle, id, AgentStatus::Meeting).await;
            }
            insert_workspace_log(
                app_handle,
                &workspace.id,
                Some(agent.id.clone()),
                "meeting",
                format!(
                    "会议开始，议题：「{}」，主持人：{}，发言顺序：{}，发言上限 {} 条",
                    topic, agent.name, order_display, max_speeches
                ),
            )
            .await;

            for a in &others {
                send_workspace_message(
                    app_handle,
                    &workspace.id,
                    "system",
                    &a.id,
                    &format!(
                        "【会议邀请】「{}」发起了会议，议题：「{}」。请立即调用 workspace_meeting_checkin \
                         工具签到参会：meeting_id 填 \"{}\"，content 留空。签到后工具会挂起等待，轮到你\
                         发言或会议结束时才会返回结果（结果里会附上你错过的全部发言）；返回后按结果里的 \
                         instruction 继续。",
                        agent.name, topic, meeting_id
                    ),
                )
                .await;
            }

            let cfg = MeetingConfig {
                meeting_id: meeting_id.clone(),
                workspace_id: workspace.id.clone(),
                topic,
                initiator_id: agent.id.clone(),
                participants,
                max_speeches,
                sleeping_before,
            };
            let coordinator_app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                meeting::run_coordinator(coordinator_app, cfg, event_rx).await;
            });

            // 主持人自己的首次签到，开场发言随之入场。
            let (reply_tx, reply_rx) = oneshot::channel();
            if tx
                .send(MeetingCheckIn { agent_id: agent.id.clone(), content: Some(opening), end_meeting: false, reply: reply_tx })
                .await
                .is_err()
            {
                return serde_json::json!({ "error": "会议协调器启动失败" });
            }
            tokio::select! {
                _ = cancel.cancelled() => serde_json::json!({ "error": "工作组已被删除" }),
                result = reply_rx => match result {
                    Ok(v) => v,
                    Err(_) => serde_json::json!({ "error": "会议已终止" }),
                },
            }
        }
        "workspace_meeting_checkin" => {
            let meeting_id = match call.arguments.get("meeting_id").and_then(|v| v.as_str()).map(str::trim) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => return serde_json::json!({ "error": "缺少 meeting_id" }),
            };
            let tx = {
                let meetings = app_handle.state::<MeetingsState>();
                let map = meetings.0.lock().unwrap();
                match map.get(&workspace.id) {
                    Some(h) if h.meeting_id == meeting_id => h.tx.clone(),
                    Some(_) => return serde_json::json!({ "error": "meeting_id 不匹配，这场会议可能已经结束" }),
                    None => return serde_json::json!({ "error": "当前没有正在进行的会议" }),
                }
            };
            let content = call
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let end_meeting = call.arguments.get("end_meeting").and_then(|v| v.as_bool()).unwrap_or(false);
            let (reply_tx, reply_rx) = oneshot::channel();
            if tx
                .send(MeetingCheckIn { agent_id: agent.id.clone(), content, end_meeting, reply: reply_tx })
                .await
                .is_err()
            {
                return serde_json::json!({ "error": "会议已结束" });
            }
            tokio::select! {
                _ = cancel.cancelled() => serde_json::json!({ "error": "工作组已被删除" }),
                result = reply_rx => match result {
                    Ok(v) => v,
                    Err(_) => serde_json::json!({ "error": "会议已终止" }),
                },
            }
        }
        _ => {
            // 权限校验：只允许调用这个 Agent 的 mcp_server_ids 白名单里的服务器
            // 暴露出来的工具，外加 server_id "builtin" 的内置工具（网络搜索/
            // 网页抓取）——它不占用 mcp_server_ids 名额，因为不是外部服务器，
            // 设计上和聊天页一样开箱即用，不需要显式授权。以前这里直接传
            // server_id=None，call_mcp_tool 会在全部已启用服务器里搜同名工具
            // 再执行——只要提示注入诱导模型说出一个未授权服务器的工具名就能
            // 绕过 Agent 的工具授权范围，所以非内置工具仍必须命中白名单。
            let db_state = app_handle.state::<DbState>();
            let owning_tool = match get_all_mcp_tools(db_state.clone()).await {
                Ok(all_tools) => all_tools
                    .into_iter()
                    .find(|t| t.name == call.name && (t.server_id == "builtin" || agent.mcp_server_ids.contains(&t.server_id))),
                Err(e) => {
                    return serde_json::json!({ "error": format!("获取 MCP 工具列表失败: {}", e) });
                }
            };
            let Some(tool) = owning_tool else {
                return serde_json::json!({ "error": format!("工具 {} 不存在，或不在该 Agent 被授权使用的 MCP 服务器范围内", call.name) });
            };
            // 无人值守缺口的另一半：权限旁路堵死了不代表安全。但给每一次工具
            // 调用都要求批准会把"自主运行"变成"事事都要盯着"，反而没人真的
            // 会去审批——只挑名字/描述命中危险关键词（删除、写入、执行命令等）
            // 的工具走审批，其余照常自动放行；用户也可以直接关掉这个开关，
            // 全部自动放行（自担风险）。
            if agent.require_tool_approval && is_dangerous_tool(&tool.name, &tool.description) {
                let approved = request_tool_approval(app_handle, &workspace.id, agent, &call.name, &call.arguments, cancel).await;
                if !approved {
                    return serde_json::json!({ "error": format!("用户未批准执行工具 {}", call.name) });
                }
            }
            match call_mcp_tool(db_state, Some(tool.server_id), call.name.clone(), call.arguments.clone()).await {
                Ok(v) => v,
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            }
        }
    }
}

/// 注册一条待处理的提议，通知前端，然后阻塞等待（只阻塞这一次工具调用的
/// 处理流程，不影响应用其他部分），直到用户通过 `workspace_resolve_proposal`
/// 批准/拒绝、10 分钟无响应超时、或者工作组/Agent 在此期间被删除
/// （`cancel` 触发）。如果不跟 `cancel` 做 race，这个等待原本会一直占着这个
/// 任务，最长撑满整整 10 分钟超时，删除之后还在往一个已经不存在的工作组
/// 里写日志。
async fn propose_agent_creation(
    app_handle: &AppHandle,
    pending: &PendingProposals,
    workspace_id: &str,
    proposing_agent: &WorkspaceAgent,
    draft: CreateAgentRequest,
    cancel: &CancellationToken,
) -> serde_json::Value {
    let proposal_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let mut map = pending.0.lock().unwrap();
        map.insert(proposal_id.clone(), tx);
    }

    let _ = app_handle.emit(
        "workspace://agent-proposal",
        serde_json::json!({
            "proposalId": proposal_id,
            "workspaceId": workspace_id,
            "proposedByAgentId": proposing_agent.id,
            "proposedByAgentName": proposing_agent.name,
            "draft": draft,
            "createdAt": Utc::now().timestamp_millis(),
        }),
    );

    record_pending_event(
        app_handle,
        workspace_id,
        &proposing_agent.id,
        &proposing_agent.name,
        "proposal",
        serde_json::json!({ "draft": draft, "proposalId": proposal_id }),
        &proposal_id,
    )
    .await;

    insert_workspace_log(
        app_handle,
        workspace_id,
        Some(proposing_agent.id.clone()),
        "agent_proposal",
        format!(
            "{} 提议创建新 Agent「{}」（{} / {}），等待用户确认",
            proposing_agent.name, draft.name, draft.provider, draft.model
        ),
    )
    .await;

    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            pending.0.lock().unwrap().remove(&proposal_id);
            None
        }
        result = tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx) => Some(result),
    };

    let response = match outcome {
        None => {
            mark_pending_event_resolved(app_handle, &proposal_id).await;
            return serde_json::json!({ "status": "cancelled", "message": "工作组已被删除" });
        }
        Some(Ok(Ok(ProposalDecision::Approved(final_request)))) => {
            let agent_handles = app_handle.state::<WorkspaceState>().0.clone();
            match spawn_agent_internal(app_handle, agent_handles, *final_request).await {
                Ok(agent) => serde_json::json!({ "status": "approved", "agentId": agent.id, "name": agent.name }),
                Err(e) => serde_json::json!({ "status": "error", "message": e.to_string() }),
            }
        }
        Some(Ok(Ok(ProposalDecision::Rejected))) => serde_json::json!({ "status": "rejected_by_user" }),
        Some(Ok(Err(_))) => serde_json::json!({ "status": "rejected_by_user" }),
        Some(Err(_)) => {
            pending.0.lock().unwrap().remove(&proposal_id);
            serde_json::json!({ "status": "timed_out", "message": "用户在 10 分钟内没有响应，提议已取消" })
        }
    };
    mark_pending_event_resolved(app_handle, &proposal_id).await;
    response
}

/// 注册一条待处理的休眠请求，告知主 Agent（同时唤醒它），然后阻塞等待，
/// 直到主 Agent 调用 `workspace_approve_sleep`/`workspace_reject_sleep`、
/// 用户通过 `workspace_resolve_sleep_request` 越权处理、10 分钟无响应超时
/// （保持清醒是比悄无声息地让 Agent 沉默下去更安全的默认行为），或者
/// `cancel` 因工作组/Agent 被删除而触发。
async fn request_sleep_approval(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent: &WorkspaceAgent,
    reason: &str,
    cancel: &CancellationToken,
) -> bool {
    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let pending = app_handle.state::<PendingSleepRequests>();
        pending.0.lock().unwrap().insert(request_id.clone(), tx);
    }

    let _ = app_handle.emit(
        "workspace://sleep-request",
        serde_json::json!({
            "requestId": request_id, "workspaceId": workspace_id,
            "agentId": agent.id, "agentName": agent.name, "reason": reason,
            "createdAt": Utc::now().timestamp_millis(),
        }),
    );

    record_pending_event(
        app_handle,
        workspace_id,
        &agent.id,
        &agent.name,
        "sleep",
        serde_json::json!({ "reason": reason, "requestId": request_id }),
        &request_id,
    )
    .await;

    if let Some(main_id) = find_main_agent_id(app_handle, workspace_id).await {
        send_workspace_message(
            app_handle,
            workspace_id,
            "system",
            &main_id,
            &format!(
                "Agent「{}」请求进入休眠状态（request_id={}），原因：{}。如果同意，调用 workspace_approve_sleep \
                 工具并填入这个 request_id；如果不同意，调用 workspace_reject_sleep。",
                agent.name, request_id, reason
            ),
        )
        .await;
    }
    insert_workspace_log(app_handle, workspace_id, Some(agent.id.clone()), "sleep_request", format!("{} 申请休眠：{}", agent.name, reason))
        .await;

    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            app_handle.state::<PendingSleepRequests>().0.lock().unwrap().remove(&request_id);
            None
        }
        result = tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx) => Some(result),
    };

    let approved = match outcome {
        None => {
            mark_pending_event_resolved(app_handle, &request_id).await;
            return false;
        }
        Some(Ok(Ok(approved))) => approved,
        Some(Ok(Err(_))) => false,
        Some(Err(_)) => {
            app_handle.state::<PendingSleepRequests>().0.lock().unwrap().remove(&request_id);
            false
        }
    };
    mark_pending_event_resolved(app_handle, &request_id).await;
    approved
}

/// 注册一条待处理的问题，通知前端弹出答题卡片，然后阻塞等待，直到用户
/// 通过 `workspace_resolve_question` 回答、10 分钟无响应超时、或者
/// `cancel` 因工作组/Agent 被删除而触发。
async fn request_user_answer(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent: &WorkspaceAgent,
    question: &str,
    cancel: &CancellationToken,
) -> String {
    let question_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let pending = app_handle.state::<PendingQuestions>();
        pending.0.lock().unwrap().insert(question_id.clone(), tx);
    }

    let _ = app_handle.emit(
        "workspace://question",
        serde_json::json!({
            "questionId": question_id, "workspaceId": workspace_id,
            "agentId": agent.id, "agentName": agent.name, "question": question,
            "createdAt": Utc::now().timestamp_millis(),
        }),
    );
    record_pending_event(
        app_handle,
        workspace_id,
        &agent.id,
        &agent.name,
        "question",
        serde_json::json!({ "question": question, "questionId": question_id }),
        &question_id,
    )
    .await;
    insert_workspace_log(app_handle, workspace_id, Some(agent.id.clone()), "question", format!("{} 提问：{}", agent.name, question)).await;

    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            app_handle.state::<PendingQuestions>().0.lock().unwrap().remove(&question_id);
            None
        }
        result = tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx) => Some(result),
    };

    let answer = match outcome {
        None => {
            mark_pending_event_resolved(app_handle, &question_id).await;
            return "（工作组已被删除）".to_string();
        }
        Some(Ok(Ok(answer))) => {
            // 把回答落库成一条静默消息（不触发唤醒——回答已经通过工具结果送进
            // 当前这次唤醒了）：时间线上问答俱全，Agent 之后的唤醒按消息表重建
            // 历史时，用户说过的话也不会凭空消失。
            let brief: String = question.chars().take(40).collect();
            let ellipsis = if question.chars().count() > 40 { "…" } else { "" };
            send_workspace_message_silent(
                app_handle,
                workspace_id,
                "user",
                &agent.id,
                &format!("（回答提问「{}{}」）{}", brief, ellipsis, answer),
            )
            .await;
            answer
        }
        Some(Ok(Err(_))) => "（用户没有回答）".to_string(),
        Some(Err(_)) => {
            app_handle.state::<PendingQuestions>().0.lock().unwrap().remove(&question_id);
            insert_workspace_log(
                app_handle,
                workspace_id,
                Some(agent.id.clone()),
                "question",
                format!("{} 的提问超时未获回答", agent.name),
            )
            .await;
            "（用户在限定时间内没有回答）".to_string()
        }
    };
    mark_pending_event_resolved(app_handle, &question_id).await;
    answer
}

/// "这个 MCP 工具一旦用错真的会造成实质损害"的关键词启发式判断——对工具的
/// 名字和描述做匹配（不区分大小写，子串匹配）。刻意只覆盖破坏性/修改性/
/// 执行类动词，不包括只读的（list/get/search/query/read/fetch 之类）——
/// 那些继续自动放行，不然审批门就会变成一堆没人真的会看、只会无脑点"同意"
/// 的噪音。中英文动词都收录了，因为 MCP 工具描述两种语言都可能出现。
const DANGEROUS_TOOL_KEYWORDS: &[&str] = &[
    "delete", "remove", "drop", "truncate", "wipe", "format", "destroy", "purge",
    "kill", "terminate", "uninstall", "shutdown", "reboot",
    "exec", "execute", "eval", "shell", "command", "spawn", "subprocess", "run_code", "runcode",
    "write", "overwrite", "modify", "move", "rename", "chmod", "chown",
    "删除", "移除", "清空", "格式化", "销毁", "卸载", "关机", "重启",
    "执行命令", "执行代码", "运行命令", "运行代码", "写入", "覆盖", "修改文件", "移动文件", "重命名",
];

fn is_dangerous_tool(name: &str, description: &str) -> bool {
    let haystack = format!("{} {}", name, description).to_lowercase();
    DANGEROUS_TOOL_KEYWORDS.iter().any(|kw| {
        if kw.is_ascii() {
            // 英文关键词要求整词命中："skill" 不该因为包含 "kill" 被拦下，
            // "commands" 也不该被 "command" 误伤。下划线/连字符/空格/中文
            // 字符都算词边界，所以 "kill_process"、"write_file" 照常命中。
            contains_ascii_word(&haystack, kw)
        } else {
            // 中文没有词边界可言，维持子串匹配。
            haystack.contains(kw)
        }
    })
}

fn contains_ascii_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    haystack.match_indices(word).any(|(pos, _)| {
        let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        let end = pos + word.len();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        before_ok && after_ok
    })
}

/// 注册一条待处理的 MCP 工具调用审批，通知前端，然后阻塞等待，直到用户
/// 批准/拒绝、10 分钟无响应超时、或者 `cancel` 触发。跟休眠审批的默认行为
/// （保持清醒是安全的）不同，这里超时/取消时默认是**拒绝**——一次真实的
/// 工具调用（可能涉及有文件系统/shell 能力的 MCP 服务器）如果没人处理，
/// 绝不能悄悄放行。
async fn request_tool_approval(
    app_handle: &AppHandle,
    workspace_id: &str,
    agent: &WorkspaceAgent,
    tool_name: &str,
    arguments: &serde_json::Value,
    cancel: &CancellationToken,
) -> bool {
    let approval_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let pending = app_handle.state::<PendingToolApprovals>();
        pending.0.lock().unwrap().insert(approval_id.clone(), tx);
    }

    let _ = app_handle.emit(
        "workspace://tool-approval",
        serde_json::json!({
            "approvalId": approval_id, "workspaceId": workspace_id,
            "agentId": agent.id, "agentName": agent.name,
            "toolName": tool_name, "arguments": arguments,
            "createdAt": Utc::now().timestamp_millis(),
        }),
    );
    record_pending_event(
        app_handle,
        workspace_id,
        &agent.id,
        &agent.name,
        "tool_approval",
        serde_json::json!({ "toolName": tool_name, "arguments": arguments, "approvalId": approval_id }),
        &approval_id,
    )
    .await;
    insert_workspace_log(
        app_handle,
        workspace_id,
        Some(agent.id.clone()),
        "tool_approval",
        format!("{} 请求执行工具「{}」，等待用户批准", agent.name, tool_name),
    )
    .await;

    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            app_handle.state::<PendingToolApprovals>().0.lock().unwrap().remove(&approval_id);
            None
        }
        result = tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx) => Some(result),
    };

    let approved = match outcome {
        None => {
            mark_pending_event_resolved(app_handle, &approval_id).await;
            return false;
        }
        Some(Ok(Ok(approved))) => approved,
        Some(Ok(Err(_))) => false,
        Some(Err(_)) => {
            app_handle.state::<PendingToolApprovals>().0.lock().unwrap().remove(&approval_id);
            false
        }
    };
    mark_pending_event_resolved(app_handle, &approval_id).await;
    approved
}

#[cfg(test)]
mod danger_classifier_tests {
    use super::is_dangerous_tool;

    #[test]
    fn flags_destructive_and_execution_tools() {
        assert!(is_dangerous_tool("delete_file", "Delete a file from disk"));
        assert!(is_dangerous_tool("shell_exec", "Run an arbitrary shell command"));
        assert!(is_dangerous_tool("write_file", "写入文件内容"));
        assert!(is_dangerous_tool("rm_dir", "递归删除目录"));
        assert!(is_dangerous_tool("format_disk", "格式化磁盘分区"));
    }

    #[test]
    fn leaves_read_only_tools_unflagged() {
        assert!(!is_dangerous_tool("list_files", "列出目录下的文件"));
        assert!(!is_dangerous_tool("search_web", "Search the web for a query"));
        assert!(!is_dangerous_tool("get_weather", "查询天气预报"));
        assert!(!is_dangerous_tool("read_file", "读取文件内容"));
    }

    #[test]
    fn word_boundary_avoids_substring_false_positives() {
        // "skill" 里包含 "kill"、"commands" 里包含 "command"——整词匹配后
        // 这些不该再被误判成危险工具。
        assert!(!is_dangerous_tool("list_skills", "List all available skills"));
        assert!(!is_dangerous_tool("get_commands_help", "Show available commands reference"));
        // 但下划线/空格分隔的真实危险动词照常命中。
        assert!(is_dangerous_tool("kill_process", "Kill a running process by pid"));
        assert!(is_dangerous_tool("run_command", "Run a shell command"));
    }
}
