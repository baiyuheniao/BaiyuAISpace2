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
    /// 用户手动暂停，或触发了唤醒频率护栏被自动暂停；暂停期间新消息仍会
    /// 存进收件箱，但不会触发处理，直到用户手动 `workspace_resume_agent`。
    Paused,
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
            AgentStatus::Paused => "paused",
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
            "paused" => AgentStatus::Paused,
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
    /// Reranker 配置 id（对应前端 settings.rerankerApiConfigs，用于唤醒时按 id
    /// 从密钥链取 API key）；None 表示不启用 reranker。
    #[serde(default)]
    pub rag_reranker_config_id: Option<String>,
    #[serde(default)]
    pub rag_reranker_base_url: Option<String>,
    #[serde(default)]
    pub rag_reranker_model: Option<String>,
    /// 精排后保留条数；None 时默认等于 rag_top_k。
    #[serde(default)]
    pub rag_rerank_top_n: Option<i32>,
    /// 跨唤醒保留的私有备忘（由 workspace_scratchpad 工具读写），每次唤醒都会
    /// 拼进系统提示词，弥补"每次醒来只记得群聊记录"的工作记忆缺失。
    #[serde(default)]
    pub scratchpad: String,
    /// 是否启用高风险 MCP 工具的审批门（见 `commands::is_dangerous_tool`）。
    /// 默认 true：只有名字/描述命中危险关键词（删除、写入、执行命令等）的
    /// 工具才需要用户批准，其余工具照常自动放行，兼顾安全和效率；
    /// false 时该 Agent 的所有工具调用都自动放行，风险自担。
    #[serde(default = "default_require_tool_approval")]
    pub require_tool_approval: bool,
    /// 是否为这个 Agent 的请求带上思考/推理参数（Anthropic extended thinking /
    /// Gemini thinkingConfig / SiliconFlow enable_thinking，按 provider 各自的
    /// 形状，见 llm.rs::run_turn）。默认关闭——会增加延迟和 token 消耗，不是
    /// 所有任务都需要，用户按 Agent 自行选择打开。
    #[serde(default)]
    pub enable_thinking: bool,
    /// 软删除时间戳；非 None 表示这个 Agent 已被用户删除，但消息/日志历史里
    /// 引用它的记录仍需要能正确显示名字，所以不做物理删除。
    #[serde(default)]
    pub deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn default_require_tool_approval() -> bool {
    true
}

/// One entry in an agent's structured to-do list, distinct from the
/// free-form `scratchpad` -- covers the "没有任务清单" half of the working-memory
/// gap, where scratchpad only covers free-text notes. Managed by the
/// `workspace_task_list` tool (add/complete/remove/list).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAgentTask {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub done: bool,
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
    #[serde(default)]
    pub rag_reranker_config_id: Option<String>,
    #[serde(default)]
    pub rag_reranker_base_url: Option<String>,
    #[serde(default)]
    pub rag_reranker_model: Option<String>,
    #[serde(default)]
    pub rag_rerank_top_n: Option<i32>,
    #[serde(default = "default_require_tool_approval")]
    pub require_tool_approval: bool,
    #[serde(default)]
    pub enable_thinking: bool,
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
    #[serde(default)]
    pub rag_reranker_config_id: Option<String>,
    #[serde(default)]
    pub rag_reranker_base_url: Option<String>,
    #[serde(default)]
    pub rag_reranker_model: Option<String>,
    #[serde(default)]
    pub rag_rerank_top_n: Option<i32>,
    #[serde(default = "default_require_tool_approval")]
    pub require_tool_approval: bool,
    #[serde(default)]
    pub enable_thinking: bool,
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
