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

/// 标识一个 Agent 是工作组的主 Agent（负责跟用户对接任务、提议创建新的子
/// Agent）还是普通子 Agent。
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

/// Agent 当前的高层状态，在前端展示为状态图标。`Sleeping`/`Meeting`/
/// `WaitingApproval`/`WaitingAnswer` 是为 Phase 2/3（休眠、提问、会议）预留的，
/// Phase 1 的循环还不会设置它们，那时只在 `Idle` 和 `Running`（或 `Error`）
/// 之间切换。
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
    /// 同一工作组里同时能存在的 Agent 数量上限（安全阀），防止主 Agent 无节制
    /// 地创建子 Agent。按工作组各自配置，创建时默认给一个保守值。
    pub max_agents: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 一个 Agent 的配置 + 持久化的运行时状态。这里刻意不重新定义工具/知识库/
/// Skill 的配置——只存引用（`mcp_server_ids`、`knowledge_base_ids`、
/// `active_skill_ids`），指向普通聊天模式已有的配置表，跟 `KnowledgeBase`
/// 只存 `embedding_api_config_id` 而不另存一份 embedding 服务商密钥是同一个
/// 思路。
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
    /// 指向前端 `ApiConfig` 列表里的一项；真正的密钥在调用时通过这个 id 从
    /// 操作系统密钥链取出，跟 `knowledge_base::commands` 里的
    /// `get_embedding_api_key` 是同一套做法。
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
    /// 单次唤醒允许的最大工具调用轮数（会议签到轮不计入）。原为写死的
    /// 常量 8，现按 Agent 可配，默认 20；配额烧完仍有无工具强制收尾轮兜底。
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: i32,
    /// 每次唤醒回放的消息历史条数上限。原为写死的常量 40，现按 Agent 可配。
    #[serde(default = "default_history_limit")]
    pub history_limit: i32,
    /// 单轮回复的最大输出 token 数；None 时沿用各 provider 的宽裕默认值
    /// （Anthropic 32000，其余不设限）。
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// 高风险工具审批的按工具白名单：名单内的工具对该 Agent 永久放行，
    /// 不再弹审批卡片。由审批卡片上的"记住选择"写入，编辑表单可撤销。
    #[serde(default)]
    pub tool_whitelist: Vec<String>,
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

pub fn default_max_tool_rounds() -> i32 {
    20
}

pub fn default_history_limit() -> i32 {
    40
}

/// Agent 结构化待办清单里的一项，跟自由格式的 `scratchpad` 不是一回事——
/// scratchpad 只能记自由文本笔记，覆盖不了"没有任务清单"这半个工作记忆缺口，
/// 这里补上。由 `workspace_task_list` 工具管理（增加/完成/删除/列出）。
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

/// 工作组共享收件箱里的一条消息。`from_agent_id`/`to_agent_id` 的取值要么是
/// 真实的 agent id，要么是字面量 `"user"`，要么是字面量 `"all"`（广播给工作组
/// 里所有 Agent）。
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

/// 共享时间线上的一条记录（消息发送、Agent 创建、状态变化、工具调用等），
/// 作为一份按时间排序的日志展示给用户。
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

/// 两条创建路径共用这个结构：手动创建的 Tauri command，以及主 Agent 的
/// `workspace_create_agent` 工具（后者要等用户在前端确认卡片里批准了提议的
/// 值之后，才会走到 `spawn_agent_internal`）。
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
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: i32,
    #[serde(default = "default_history_limit")]
    pub history_limit: i32,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub tool_whitelist: Vec<String>,
}

pub fn default_rag_top_k() -> i32 {
    5
}

pub fn default_rag_retrieval_mode() -> String {
    "hybrid".to_string()
}

/// 一个已存在 Agent 的可编辑字段（`workspace_update_agent`）。刻意不包含
/// `role`/`workspace_id`——把一个运行中的 Agent 在 main/sub 之间切换，会让它
/// 已经发出的"工具是否可用"这类假设失效；把它挪到另一个工作组也没有明确
/// 语义定义；这些少见场景直接删了重建即可。其余字段都可以放心热更新，因为
/// `process_agent_wake` 每次唤醒都会重新从数据库读一遍这个 Agent 的行。
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
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: i32,
    #[serde(default = "default_history_limit")]
    pub history_limit: i32,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub tool_whitelist: Vec<String>,
}

/// 一个正在等待人工决策的 `workspace_create_agent` 提议 / `workspace_sleep`
/// 请求 / `workspace_asks` 问题，需要持久化，这样即便应用重启、或者事件触发时
/// 用户根本没打开这个页面，请求也不会丢（早先这些东西只存在于内存里的
/// oneshot channel 加一次性前端事件——一旦错过那个事件，请求就永久消失了，
/// 即便对应的 Agent 其实还卡在原地等结果）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePendingEvent {
    pub id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub agent_name: String,
    /// "proposal" | "sleep" | "question"
    pub kind: String,
    /// 按类型不同而不同的字段，以 JSON 形式存放：proposal 带 `draft`；sleep 带
    /// `reason`；question 带 `question`。
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

pub const DEFAULT_MAX_AGENTS: i32 = 5;
