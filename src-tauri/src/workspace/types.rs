// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Workspace not found: {0}")]
    NotFound(String),
    #[error("Agent not found: {0}")]
    AgentNotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Workspace agent limit reached: {0}")]
    AgentLimitReached(String),
    #[error("LLM error: {0}")]
    LlmError(String),
}

impl Serialize for WorkspaceError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

impl From<rusqlite::Error> for WorkspaceError {
    fn from(e: rusqlite::Error) -> Self {
        WorkspaceError::DatabaseError(e.to_string())
    }
}

/// Whether an agent is the workspace's main agent (negotiates tasks with the
/// user and proposes new sub-agents) or an ordinary sub-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Main,
    Sub,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentRole::Main => "main",
            AgentRole::Sub => "sub",
        }
    }

    pub fn from_str(s: &str) -> Self {
        if s == "main" {
            AgentRole::Main
        } else {
            AgentRole::Sub
        }
    }
}

/// An agent's current high-level state, shown in the frontend as a status
/// icon. `Sleeping`/`Meeting`/`WaitingApproval`/`WaitingAnswer` are reserved
/// for Phase 2/3 (休眠, 提问, 会议) and not yet set by the Phase 1 loop, which
/// only ever moves between `Idle` and `Running` (or `Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Running,
    WaitingApproval,
    WaitingAnswer,
    Sleeping,
    Meeting,
    Error,
}

impl AgentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Running => "running",
            AgentStatus::WaitingApproval => "waiting_approval",
            AgentStatus::WaitingAnswer => "waiting_answer",
            AgentStatus::Sleeping => "sleeping",
            AgentStatus::Meeting => "meeting",
            AgentStatus::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "running" => AgentStatus::Running,
            "waiting_approval" => AgentStatus::WaitingApproval,
            "waiting_answer" => AgentStatus::WaitingAnswer,
            "sleeping" => AgentStatus::Sleeping,
            "meeting" => AgentStatus::Meeting,
            "error" => AgentStatus::Error,
            _ => AgentStatus::Idle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Safety cap on how many agents can exist in this workspace at once,
    /// so a main agent can't create sub-agents without bound. Configurable
    /// per-workspace, defaults to a conservative value at creation time.
    pub max_agents: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One agent's configuration + persisted runtime status. Deliberately does
/// not redefine tool/knowledge-base/skill configuration -- it only stores
/// references (`mcp_server_ids`, `knowledge_base_ids`, `active_skill_ids`)
/// into the regular chat mode's existing config tables, same as how
/// `KnowledgeBase` stores `embedding_api_config_id` rather than its own copy
/// of the embedding provider's secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAgent {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub role: AgentRole,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// References an entry in the frontend's `ApiConfig` list; the actual
    /// secret is fetched from the OS keyring via this id at call time,
    /// mirroring `get_embedding_api_key` in `knowledge_base::commands`.
    pub api_config_id: String,
    pub system_prompt: String,
    pub mcp_server_ids: Vec<String>,
    pub knowledge_base_ids: Vec<String>,
    pub active_skill_ids: Vec<String>,
    pub status: AgentStatus,
    /// RAG 检索的 top_k，之前在 build_agent_system_prompt 里硬编码为 5。
    pub rag_top_k: i32,
    /// RAG 检索模式，之前硬编码为 "hybrid"；取值 "vector"/"keyword"/"hybrid"。
    pub rag_retrieval_mode: String,
    /// 跨唤醒保留的私有备忘（由 workspace_scratchpad 工具读写），每次唤醒都会
    /// 拼进系统提示词，弥补"每次醒来只记得群聊记录"的工作记忆缺失。
    #[serde(default)]
    pub scratchpad: String,
    /// 软删除时间戳；非 None 表示这个 Agent 已被用户删除，但消息/日志历史里
    /// 引用它的记录仍需要能正确显示名字，所以不做物理删除。
    #[serde(default)]
    pub deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One message in a workspace's shared inbox. `from_agent_id`/`to_agent_id`
/// hold either a real agent id, the literal `"user"`, or the literal `"all"`
/// (broadcast to every agent in the workspace).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMessage {
    pub id: String,
    pub workspace_id: String,
    pub from_agent_id: String,
    pub to_agent_id: String,
    pub content: String,
    pub created_at: i64,
}

/// One shared timeline entry (message sent, agent created, status changed,
/// tool called, etc.), shown to the user as a single chronological log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLogEntry {
    pub id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub description: String,
    pub max_agents: Option<i32>,
}

/// Shared by both creation paths: the manual-creation Tauri command and the
/// main agent's `workspace_create_agent` tool (the latter only reaches
/// `spawn_agent_internal` after the user approves the proposed values via
/// the frontend confirmation card).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentRequest {
    pub workspace_id: String,
    pub name: String,
    pub role: AgentRole,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_config_id: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub mcp_server_ids: Vec<String>,
    #[serde(default)]
    pub knowledge_base_ids: Vec<String>,
    #[serde(default)]
    pub active_skill_ids: Vec<String>,
    #[serde(default = "default_rag_top_k")]
    pub rag_top_k: i32,
    #[serde(default = "default_rag_retrieval_mode")]
    pub rag_retrieval_mode: String,
}

pub fn default_rag_top_k() -> i32 {
    5
}

pub fn default_rag_retrieval_mode() -> String {
    "hybrid".to_string()
}

/// Editable fields for an existing agent (`workspace_update_agent`). Deliberately
/// omits `role`/`workspace_id` -- switching a running agent between main/sub
/// makes its already-issued tool-availability assumptions stale, and moving it
/// to another workspace has no defined semantics; delete-and-recreate covers
/// those rare cases. Everything else here is safe to change live because
/// `process_agent_wake` reloads the agent's row fresh from the DB every wake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentRequest {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_config_id: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub mcp_server_ids: Vec<String>,
    #[serde(default)]
    pub knowledge_base_ids: Vec<String>,
    #[serde(default)]
    pub active_skill_ids: Vec<String>,
    #[serde(default = "default_rag_top_k")]
    pub rag_top_k: i32,
    #[serde(default = "default_rag_retrieval_mode")]
    pub rag_retrieval_mode: String,
}

/// A `workspace_create_agent` proposal / `workspace_sleep` request /
/// `workspace_asks` question that's waiting on a human decision, persisted so
/// it survives an app restart or a user simply not having the page open when
/// it fired (previously these lived only as in-memory oneshot channels plus a
/// fire-and-forget frontend event -- miss the event and the request was gone
/// for good even though the agent was still blocked waiting on it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePendingEvent {
    pub id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub agent_name: String,
    /// "proposal" | "sleep" | "question"
    pub kind: String,
    /// Type-specific fields as JSON: proposal carries `draft`; sleep carries
    /// `reason`; question carries `question`.
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

pub const DEFAULT_MAX_AGENTS: i32 = 5;
