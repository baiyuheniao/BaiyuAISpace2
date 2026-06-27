// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::db;
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
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const PROPOSAL_TIMEOUT_SECS: u64 = 600;
const MAX_ROUNDS_PER_WAKE: u32 = 8;
const MAX_HISTORY_MESSAGES: i64 = 40;

/// One running agent's wake signal + stop switch. Lives only in memory --
/// restarting the app currently does not resume agent loops, only their
/// persisted DB state (Phase 1 scope; auto-resume on launch is not yet
/// implemented).
pub struct AgentHandle {
    pub notify: Arc<Notify>,
    pub cancel: CancellationToken,
}

/// Registry of running agent loops, keyed by agent id (ids are UUIDs and
/// therefore globally unique, so a flat map works fine across workspaces).
#[derive(Default)]
pub struct WorkspaceState(pub Arc<Mutex<HashMap<String, AgentHandle>>>);

/// What the user decided about a main agent's `workspace_create_agent`
/// proposal. `Approved` carries the *final* request, filled in by the
/// frontend confirmation card (it supplies `api_config_id`/`base_url`,
/// which the model can't know).
pub enum ProposalDecision {
    Approved(Box<CreateAgentRequest>),
    Rejected,
}

#[derive(Default)]
pub struct PendingProposals(pub Arc<Mutex<HashMap<String, oneshot::Sender<ProposalDecision>>>>);

/// A sub-agent's pending `workspace_sleep` request, keyed by a generated
/// request id. Resolved either by the main agent calling
/// `workspace_approve_sleep`/`workspace_reject_sleep`, or by the user
/// overriding directly via `workspace_resolve_sleep_request` -- whichever
/// happens first wins, since removing the entry from the map is what grants
/// the right to resolve it.
#[derive(Default)]
pub struct PendingSleepRequests(pub Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>);

/// A pending `workspace_asks` question, keyed by a generated question id.
/// Resolved by the user answering via `workspace_resolve_question`.
#[derive(Default)]
pub struct PendingQuestions(pub Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>);

/// Per-agent meeting-turn completion signals, keyed by agent id. When the
/// meeting coordinator (`run_meeting`) is waiting for a specific agent's
/// speech, it inserts a sender here. After `process_agent_wake` produces the
/// agent's text output, it removes the entry and fires the signal, unblocking
/// the coordinator so it can move on to the next speaker.
#[derive(Default)]
pub struct PendingMeetingTurns(pub Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>);

pub fn init_workspace_tables(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    db::init_workspace_tables(conn)
}

// ---------------------------------------------------------------------------
// Tauri commands
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
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::insert_workspace(&conn, &workspace)?;
    Ok(workspace)
}

#[tauri::command]
pub async fn workspace_list(db_state: State<'_, DbState>) -> Result<Vec<Workspace>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::list_workspaces(&conn)
}

#[tauri::command]
pub async fn workspace_delete(
    workspace_id: String,
    db_state: State<'_, DbState>,
    workspace_state: State<'_, WorkspaceState>,
) -> Result<(), WorkspaceError> {
    let agent_ids: Vec<String> = {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
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

    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::delete_workspace(&conn, &workspace_id)?;
    Ok(())
}

/// Manual-creation path: the user fills in a form and this directly spawns
/// the agent and its background loop. The other creation path -- the main
/// agent's `workspace_create_agent` tool -- funnels through the same
/// `spawn_agent_internal` after the user approves the proposal via
/// `workspace_resolve_proposal`, so both paths end up identical from here on.
#[tauri::command]
pub async fn workspace_create_agent_manual(
    request: CreateAgentRequest,
    app_handle: AppHandle,
    workspace_state: State<'_, WorkspaceState>,
) -> Result<WorkspaceAgent, WorkspaceError> {
    spawn_agent_internal(&app_handle, workspace_state.0.clone(), request).await
}

#[tauri::command]
pub async fn workspace_list_agents(
    workspace_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceAgent>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::list_agents(&conn, &workspace_id)
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
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::delete_agent(&conn, &agent_id)?;
    Ok(())
}

/// User sends a message into the workspace -- to one specific agent, or
/// broadcast to everyone with `to_agent_id: "all"`. This is also how a
/// freshly created agent gets its first wake: an agent's loop starts dormant
/// (blocked on its `Notify`) and only runs once something actually messages
/// it, so a brand-new agent never gets asked to reply with zero context.
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
    Ok(())
}

#[tauri::command]
pub async fn workspace_list_messages(
    workspace_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceMessage>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::list_messages(&conn, &workspace_id, 500)
}

#[tauri::command]
pub async fn workspace_list_logs(
    workspace_id: String,
    db_state: State<'_, DbState>,
) -> Result<Vec<WorkspaceLogEntry>, WorkspaceError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::list_logs(&conn, &workspace_id, 500)
}

/// The frontend calls this in response to a `workspace://agent-proposal`
/// event, after the user reviews/edits the main agent's proposed sub-agent
/// and clicks approve/reject. `request` must be the full, user-confirmed
/// `CreateAgentRequest` (including `api_config_id`/`base_url`, which the
/// model never supplied) when `approved` is true.
#[tauri::command]
pub async fn workspace_resolve_proposal(
    proposal_id: String,
    approved: bool,
    request: Option<CreateAgentRequest>,
    pending: State<'_, PendingProposals>,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&proposal_id)
    };
    let sender = sender.ok_or_else(|| {
        WorkspaceError::NotFound(format!("提议 {} 不存在或已被处理/超时", proposal_id))
    })?;

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
    Ok(())
}

/// Lets the user directly approve/reject a sub-agent's pending
/// `workspace_sleep` request, bypassing the main agent -- the "用户也能直接代为
/// 批准/拒绝" override from the design doc. Whoever removes the entry from
/// the map first (this, or the main agent calling `workspace_approve_sleep`/
/// `workspace_reject_sleep`) is the one whose decision actually takes effect.
#[tauri::command]
pub async fn workspace_resolve_sleep_request(
    request_id: String,
    approved: bool,
    pending: State<'_, PendingSleepRequests>,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&request_id)
    };
    let sender = sender.ok_or_else(|| {
        WorkspaceError::NotFound(format!("休眠请求 {} 不存在或已被处理/超时", request_id))
    })?;
    let _ = sender.send(approved);
    Ok(())
}

/// The frontend calls this after the user types an answer into the card
/// popped up in response to a `workspace://question` event.
#[tauri::command]
pub async fn workspace_resolve_question(
    question_id: String,
    answer: String,
    pending: State<'_, PendingQuestions>,
) -> Result<(), WorkspaceError> {
    let sender = {
        let mut map = pending.0.lock().unwrap();
        map.remove(&question_id)
    };
    let sender = sender.ok_or_else(|| {
        WorkspaceError::NotFound(format!("问题 {} 不存在或已被处理/超时", question_id))
    })?;
    let _ = sender.send(answer);
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers shared by both creation paths and the agent loop
// ---------------------------------------------------------------------------

async fn load_agent(app_handle: &AppHandle, agent_id: &str) -> Result<Option<WorkspaceAgent>, WorkspaceError> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::get_agent(&conn, agent_id)
}

async fn load_workspace(app_handle: &AppHandle, workspace_id: &str) -> Result<Option<Workspace>, WorkspaceError> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;
    db::get_workspace(&conn, workspace_id)
}

async fn set_agent_status(app_handle: &AppHandle, agent_id: &str, status: AgentStatus) {
    let db_state = app_handle.state::<DbState>();
    {
        let db = db_state.0.lock().await;
        if let Ok(conn) = rusqlite::Connection::open(&db.path) {
            let _ = db::update_agent_status(&conn, agent_id, status);
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
        if let Ok(conn) = rusqlite::Connection::open(&db.path) {
            let _ = db::insert_log(&conn, &entry);
        }
    }
    let _ = app_handle.emit("workspace://log", &entry);
}

/// Persists a message and wakes whichever agent(s) it's addressed to.
/// `to_agent_id` of `"all"` broadcasts to every other agent currently
/// registered in `WorkspaceState` (not just ones in this workspace --
/// acceptable for Phase 1 since handles are looked up by id and a wake on an
/// unrelated agent is a no-op cost, but should be scoped per-workspace if
/// `WorkspaceState` ever needs to track workspace membership directly).
pub async fn send_workspace_message(
    app_handle: &AppHandle,
    workspace_id: &str,
    from_agent_id: &str,
    to_agent_id: &str,
    content: &str,
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
        if let Ok(conn) = rusqlite::Connection::open(&db.path) {
            let _ = db::insert_message(&conn, &msg);
        }
    }
    let _ = app_handle.emit("workspace://message", &msg);

    let workspace_state = app_handle.state::<WorkspaceState>();
    let handles = workspace_state.0.lock().unwrap();
    if to_agent_id == "all" {
        for (id, handle) in handles.iter() {
            if id != from_agent_id {
                handle.notify.notify_one();
            }
        }
    } else if let Some(handle) = handles.get(to_agent_id) {
        handle.notify.notify_one();
    }
}

async fn list_agents_for_workspace(app_handle: &AppHandle, workspace_id: &str) -> Vec<WorkspaceAgent> {
    let db_state = app_handle.state::<DbState>();
    let db = db_state.0.lock().await;
    match rusqlite::Connection::open(&db.path) {
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

/// After a sub-agent's sleep request gets approved, check whether every
/// sub-agent in the workspace is now `Sleeping`; if so, message the main
/// agent (which also wakes it) asking it to review whether the task is done.
async fn maybe_trigger_main_agent_review(app_handle: &AppHandle, workspace_id: &str) {
    let agents = list_agents_for_workspace(app_handle, workspace_id).await;
    let subs: Vec<_> = agents.iter().filter(|a| a.role == AgentRole::Sub).collect();
    if subs.is_empty() || !subs.iter().all(|a| a.status == AgentStatus::Sleeping) {
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
        "工作组内所有子 Agent 都已进入休眠状态，请验收当前任务进度：如果任务已经完成，用 workspace_message \
         告知用户；如果还没完成，可以用 workspace_message 叫醒某个子 Agent 继续推进，或者用 workspace_create_agent \
         创建新的 Agent。",
    )
    .await;
    insert_workspace_log(
        app_handle,
        workspace_id,
        None,
        "acceptance_review",
        "所有子 Agent 已休眠，已唤醒主 Agent 验收任务进度".to_string(),
    )
    .await;
}

/// Shared by the manual-creation command and an approved
/// `workspace_create_agent` proposal: validates the agent-count safety cap,
/// inserts the DB row, and starts the agent's background loop.
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
        let conn = rusqlite::Connection::open(&db.path).map_err(|e| WorkspaceError::DatabaseError(e.to_string()))?;

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

/// Deliberately synchronous (not `async fn`): it only ever does a quick,
/// non-blocking `std::sync::Mutex` insert before firing off the loop task.
/// Keeping it sync breaks an indirect recursive-async cycle that otherwise
/// trips up rustc's Send checking -- `run_agent_loop` can reach back here
/// through `process_agent_wake` -> `dispatch_tool_call` ->
/// `propose_agent_creation` -> `spawn_agent_internal` when a main agent's
/// proposal gets approved, and an `async fn` here would make that chain
/// self-referential.
fn start_agent_loop(app_handle: AppHandle, agent_handles: Arc<Mutex<HashMap<String, AgentHandle>>>, agent: WorkspaceAgent) {
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

/// The agent's persistent background task: sleeps on `notify` until
/// something addresses it, then processes everything that's accumulated and
/// goes back to sleep. Runs until `cancel` fires (workspace/agent deleted).
async fn run_agent_loop(
    app_handle: AppHandle,
    workspace_id: String,
    agent_id: String,
    notify: Arc<Notify>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = notify.notified() => {}
        }
        if cancel.is_cancelled() {
            break;
        }

        if let Err(e) = process_agent_wake(&app_handle, &workspace_id, &agent_id, &cancel).await {
            log::error!("Workspace agent {} 处理失败: {}", agent_id, e);
            set_agent_status(&app_handle, &agent_id, AgentStatus::Error).await;
            insert_workspace_log(&app_handle, &workspace_id, Some(agent_id.clone()), "error", e.to_string()).await;
        }
    }
    log::info!("Workspace agent {} 循环已停止", agent_id);
}

/// One "wake": reload the agent's current config, replay its relevant
/// message history, then keep alternating model-call <-> tool-execution
/// (bounded by `MAX_ROUNDS_PER_WAKE`) until the model produces a plain-text
/// reply instead of another tool call.
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

    log::info!(
        "[workspace] 唤醒 Agent「{}」({}) - workspace: {} model: {}/{}",
        agent.name, agent_id, workspace.name, agent.provider, agent.model
    );

    // Keep Meeting status visible while the agent speaks during a round-robin;
    // only promote to Running when starting a normal (non-meeting) wake.
    if agent.status != AgentStatus::Meeting {
        set_agent_status(app_handle, agent_id, AgentStatus::Running).await;
    }

    let chat_history = build_chat_history(app_handle, workspace_id, &agent).await;
    if chat_history.is_empty() {
        log::debug!("[workspace] Agent「{}」历史消息为空，跳过本次唤醒", agent.name);
        set_agent_status(app_handle, agent_id, AgentStatus::Idle).await;
        return Ok(());
    }
    let latest_query = chat_history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let system_prompt = build_agent_system_prompt(app_handle, &agent, &latest_query).await;
    let mut native_messages = build_native_messages(&agent.provider, &chat_history);

    // Local models (e.g. Ollama) don't require an API key -- mirrors the
    // same exception in llm.rs's request-level `get_api_key()`.
    let api_key = if agent.provider == "local" {
        String::new()
    } else {
        secure_storage::get_api_key(agent.api_config_id.clone())
            .map_err(|e| WorkspaceError::InvalidConfig(e.to_string()))?
            .ok_or_else(|| WorkspaceError::InvalidConfig(format!("Agent「{}」未配置 API 密钥", agent.name)))?
    };

    let mut tools = workspace_tool_defs(&agent);
    if !agent.mcp_server_ids.is_empty() {
        let db_state = app_handle.state::<DbState>();
        match get_all_mcp_tools(db_state).await {
            Ok(all_tools) => {
                tools.extend(all_tools.into_iter().filter(|t| agent.mcp_server_ids.contains(&t.server_id)));
            }
            Err(e) => log::warn!("Workspace agent {} 获取 MCP 工具列表失败: {}", agent_id, e),
        }
    }

    log::debug!(
        "[workspace] Agent「{}」开始推理 - 历史 {} 条消息，可用工具 {} 个",
        agent.name, chat_history.len(), tools.len()
    );
    let mut produced_final_text = false;
    for round in 0..MAX_ROUNDS_PER_WAKE {
        if cancel.is_cancelled() {
            return Ok(());
        }

        log::debug!("[workspace] Agent「{}」第 {} 轮推理", agent.name, round + 1);
        let outcome = run_turn(
            &agent.provider,
            &agent.model,
            &api_key,
            &agent.base_url,
            Some(&system_prompt),
            &native_messages,
            &tools,
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
                // Signal the meeting coordinator that this agent's turn is done
                // (fires only when this agent was a meeting participant waiting in run_meeting)
                let meeting_tx = app_handle.state::<PendingMeetingTurns>().0.lock().unwrap().remove(agent_id);
                if let Some(tx) = meeting_tx {
                    let _ = tx.send(());
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
                    let result = dispatch_tool_call(app_handle, &workspace, &agent, call).await;
                    log::debug!(
                        "[workspace] 工具 {} 返回: {}",
                        call.name,
                        result.to_string().chars().take(200).collect::<String>()
                    );
                    results.push(result);
                }
                append_tool_round(&agent.provider, &mut native_messages, &calls, &results);
            }
        }
    }

    if !produced_final_text {
        log::warn!(
            "Workspace agent {} 在 {} 轮内没有给出最终回复，提前结束本次唤醒",
            agent_id,
            MAX_ROUNDS_PER_WAKE
        );
    }

    // Don't stomp over Sleeping (set by workspace_sleep) or Meeting (managed
    // by run_meeting which resets participants to Idle after the meeting ends).
    let blocking = matches!(
        load_agent(app_handle, agent_id).await,
        Ok(Some(WorkspaceAgent { status: AgentStatus::Sleeping | AgentStatus::Meeting, .. }))
    );
    if !blocking {
        set_agent_status(app_handle, agent_id, AgentStatus::Idle).await;
    }
    Ok(())
}

/// Recent messages relevant to this agent, converted to the flat
/// `ChatMessage` shape `build_native_messages` expects. Incoming messages
/// are prefixed with the sender's display name so a multi-party
/// conversation stays legible to the model; the agent's own past messages
/// are left as-is since it already knows it said them.
async fn build_chat_history(app_handle: &AppHandle, workspace_id: &str, agent: &WorkspaceAgent) -> Vec<ChatMessage> {
    let db_state = app_handle.state::<DbState>();
    let (messages, agents) = {
        let db = db_state.0.lock().await;
        let conn = match rusqlite::Connection::open(&db.path) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let messages = db::list_recent_messages_for_agent(&conn, workspace_id, &agent.id, MAX_HISTORY_MESSAGES)
            .unwrap_or_default();
        let agents = db::list_agents(&conn, workspace_id).unwrap_or_default();
        (messages, agents)
    };

    let name_of = |id: &str| -> String {
        if id == "user" {
            return "用户".to_string();
        }
        if id == "all" {
            return "所有人".to_string();
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
            }
        })
        .collect()
}

/// Merges the agent's own system prompt with its active Skills' instructions
/// and, if it has knowledge bases configured, RAG context retrieved for the
/// latest incoming message -- reusing `search_knowledge_base`/`build_context`
/// exactly as the regular chat mode does, rather than re-implementing
/// retrieval here.
async fn build_agent_system_prompt(app_handle: &AppHandle, agent: &WorkspaceAgent, latest_query: &str) -> String {
    let mut sections = vec![agent.system_prompt.clone()];

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
        let db_state = app_handle.state::<DbState>();
        let kb_state = app_handle.state::<KbState>();
        for kb_id in &agent.knowledge_base_ids {
            let request = RetrievalRequest {
                kb_id: kb_id.clone(),
                query: latest_query.to_string(),
                top_k: 5,
                retrieval_mode: RetrievalMode::Hybrid,
                similarity_threshold: 0.0,
            };
            match search_knowledge_base(request, db_state.clone(), kb_state.clone()).await {
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

/// The always-available Workspace tools. `workspace_create_agent`/
/// `workspace_approve_sleep`/`workspace_reject_sleep` are main-agent-only;
/// `workspace_sleep` is sub-agent-only (the main agent is the one doing the
/// overseeing, it doesn't make sense for it to sleep); `workspace_message`/
/// `workspace_agent_list`/`workspace_asks` are available to everyone.
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
        description: "发起一次工作组会议，组内其他 Agent 会按创建先后顺序轮流就议题发言一次，全部发言完毕后返回给你做总结。".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "topic": { "type": "string", "description": "会议议题" } },
            "required": ["topic"]
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

/// Executes one tool call the model asked for, always returning a JSON
/// value (errors become `{"error": ...}` rather than propagating) since this
/// result is fed straight back to the model as the tool's output.
async fn dispatch_tool_call(
    app_handle: &AppHandle,
    workspace: &Workspace,
    agent: &WorkspaceAgent,
    call: &PendingToolCall,
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
                match rusqlite::Connection::open(&db.path) {
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
                rusqlite::Connection::open(&db.path)
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
            };

            let pending = app_handle.state::<PendingProposals>();
            propose_agent_creation(app_handle, &pending, &workspace.id, agent, draft).await
        }
        "workspace_sleep" => {
            if agent.role == AgentRole::Main {
                return serde_json::json!({ "error": "主 Agent 不能申请休眠" });
            }
            let reason = call.arguments.get("reason").and_then(|v| v.as_str()).unwrap_or("未说明").to_string();

            set_agent_status(app_handle, &agent.id, AgentStatus::WaitingApproval).await;
            let approved = request_sleep_approval(app_handle, &workspace.id, agent, &reason).await;
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
            let answer = request_user_answer(app_handle, &workspace.id, agent, &question).await;
            set_agent_status(app_handle, &agent.id, AgentStatus::Running).await;
            serde_json::json!({ "answer": answer })
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
            let topic = call.arguments.get("topic").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if topic.trim().is_empty() {
                return serde_json::json!({ "error": "topic 不能为空" });
            }
            run_meeting(app_handle, workspace, agent, topic).await
        }
        _ => {
            let db_state = app_handle.state::<DbState>();
            match call_mcp_tool(db_state, None, call.name.clone(), call.arguments.clone()).await {
                Ok(v) => v,
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            }
        }
    }
}

/// Registers a pending proposal, notifies the frontend, and blocks (only
/// this one tool call's processing -- not the rest of the app) until the
/// user approves/rejects it via `workspace_resolve_proposal`, or 10 minutes
/// pass with no response.
async fn propose_agent_creation(
    app_handle: &AppHandle,
    pending: &PendingProposals,
    workspace_id: &str,
    proposing_agent: &WorkspaceAgent,
    draft: CreateAgentRequest,
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
        }),
    );

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

    match tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx).await {
        Ok(Ok(ProposalDecision::Approved(final_request))) => {
            let agent_handles = app_handle.state::<WorkspaceState>().0.clone();
            match spawn_agent_internal(app_handle, agent_handles, *final_request).await {
                Ok(agent) => serde_json::json!({ "status": "approved", "agentId": agent.id, "name": agent.name }),
                Err(e) => serde_json::json!({ "status": "error", "message": e.to_string() }),
            }
        }
        Ok(Ok(ProposalDecision::Rejected)) => serde_json::json!({ "status": "rejected_by_user" }),
        Ok(Err(_)) => serde_json::json!({ "status": "rejected_by_user" }),
        Err(_) => {
            pending.0.lock().unwrap().remove(&proposal_id);
            serde_json::json!({ "status": "timed_out", "message": "用户在 10 分钟内没有响应，提议已取消" })
        }
    }
}

/// Registers a pending sleep request, tells the main agent about it (which
/// also wakes it), and blocks until either the main agent calls
/// `workspace_approve_sleep`/`workspace_reject_sleep` or the user overrides
/// via `workspace_resolve_sleep_request`. Defaults to *not* approved if 10
/// minutes pass with no response -- staying awake is the safer default than
/// silently letting an agent go quiet.
async fn request_sleep_approval(app_handle: &AppHandle, workspace_id: &str, agent: &WorkspaceAgent, reason: &str) -> bool {
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
        }),
    );

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

    match tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx).await {
        Ok(Ok(approved)) => approved,
        Ok(Err(_)) => false,
        Err(_) => {
            app_handle.state::<PendingSleepRequests>().0.lock().unwrap().remove(&request_id);
            false
        }
    }
}

/// Drives a round-robin meeting: sets each non-initiator agent to Meeting
/// status, sends them a "your turn" system message in creation order, and
/// waits up to 3 minutes for each to produce their speech (signalled via
/// `PendingMeetingTurns`). After all have spoken, resets them to Idle and
/// returns control to the initiator, who then produces the closing summary.
async fn run_meeting(
    app_handle: &AppHandle,
    workspace: &Workspace,
    initiator: &WorkspaceAgent,
    topic: String,
) -> serde_json::Value {
    let all_agents = list_agents_for_workspace(app_handle, &workspace.id).await;
    let mut speakers: Vec<_> = all_agents.into_iter().filter(|a| a.id != initiator.id).collect();
    speakers.sort_by_key(|a| a.created_at);

    if speakers.is_empty() {
        return serde_json::json!({ "error": "工作组内没有其他 Agent，无法召开会议" });
    }

    let order_display = std::iter::once(initiator.name.as_str())
        .chain(speakers.iter().map(|a| a.name.as_str()))
        .collect::<Vec<_>>()
        .join(" → ");

    insert_workspace_log(
        app_handle,
        &workspace.id,
        Some(initiator.id.clone()),
        "meeting",
        format!("会议开始，议题：「{}」；发言顺序：{}", topic, order_display),
    )
    .await;

    for speaker in &speakers {
        set_agent_status(app_handle, &speaker.id, AgentStatus::Meeting).await;
    }

    let meeting_state = app_handle.state::<PendingMeetingTurns>();

    for (i, speaker) in speakers.iter().enumerate() {
        let (tx, rx) = oneshot::channel::<()>();
        meeting_state.0.lock().unwrap().insert(speaker.id.clone(), tx);

        let context = if i == 0 {
            format!("会议由「{}」发起", initiator.name)
        } else {
            format!("前面已有 {} 位参与者发言", i)
        };

        send_workspace_message(
            app_handle,
            &workspace.id,
            "system",
            &speaker.id,
            &format!(
                "【会议通知】议题「{}」——{} - 轮到你发言了。\
                 请就该议题简短说明你的看法，用普通文字回复即可，无需调用工具。",
                topic, context
            ),
        )
        .await;

        match tokio::time::timeout(Duration::from_secs(180), rx).await {
            Ok(Ok(())) => {}
            _ => {
                meeting_state.0.lock().unwrap().remove(&speaker.id);
                insert_workspace_log(
                    app_handle,
                    &workspace.id,
                    Some(speaker.id.clone()),
                    "meeting",
                    format!("「{}」在会议中发言超时，已跳过", speaker.name),
                )
                .await;
            }
        }
    }

    for speaker in &speakers {
        if matches!(
            load_agent(app_handle, &speaker.id).await,
            Ok(Some(WorkspaceAgent { status: AgentStatus::Meeting, .. }))
        ) {
            set_agent_status(app_handle, &speaker.id, AgentStatus::Idle).await;
        }
    }

    insert_workspace_log(
        app_handle,
        &workspace.id,
        Some(initiator.id.clone()),
        "meeting",
        format!("会议结束，共 {} 位 Agent 参与发言，请做总结", speakers.len()),
    )
    .await;

    serde_json::json!({
        "status": "meeting_completed",
        "topic": topic,
        "participants_spoke": speakers.len(),
    })
}

/// Registers a pending question, notifies the frontend to pop up an answer
/// card, and blocks until the user answers via `workspace_resolve_question`
/// or 10 minutes pass with no response.
async fn request_user_answer(app_handle: &AppHandle, workspace_id: &str, agent: &WorkspaceAgent, question: &str) -> String {
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
        }),
    );
    insert_workspace_log(app_handle, workspace_id, Some(agent.id.clone()), "question", format!("{} 提问：{}", agent.name, question)).await;

    match tokio::time::timeout(Duration::from_secs(PROPOSAL_TIMEOUT_SECS), rx).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) => "（用户没有回答）".to_string(),
        Err(_) => {
            app_handle.state::<PendingQuestions>().0.lock().unwrap().remove(&question_id);
            "（用户在限定时间内没有回答）".to_string()
        }
    }
}
