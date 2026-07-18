// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use rusqlite::Connection;

/// 打开连接时设置 busy-timeout，这样一次写入如果撞上另一个连接短暂持有写锁
/// 的窗口，会重试最多 5 秒，而不是立刻以 `SQLITE_BUSY` 失败——本模块里每个
/// 业务操作都各自打开一个短生命周期的连接（见 `delete_workspace` 的文档注释），
/// 所以两个 Agent 的循环同时触发写入，是真实会发生的场景，不是纸上谈兵。
pub fn open_conn(path: &str) -> Result<Connection, WorkspaceError> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// 若 Workspace 相关表不存在则创建，并对老版本安装做增量迁移（`ALTER TABLE
/// ... ADD COLUMN`，执行前先查一遍 `PRAGMA table_info` 做保护）追平到当前 schema。
pub fn init_workspace_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            max_agents INTEGER NOT NULL DEFAULT 5,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_agents (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'sub',
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            base_url TEXT NOT NULL DEFAULT '',
            api_config_id TEXT NOT NULL DEFAULT '',
            system_prompt TEXT NOT NULL DEFAULT '',
            mcp_server_ids TEXT NOT NULL DEFAULT '[]',
            knowledge_base_ids TEXT NOT NULL DEFAULT '[]',
            active_skill_ids TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL DEFAULT 'idle',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_messages (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            from_agent_id TEXT NOT NULL,
            to_agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_logs (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            agent_id TEXT,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_pending_events (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            resolved_at INTEGER
        )
        "#,
        [],
    )?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_agent_tasks (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL REFERENCES workspace_agents(id) ON DELETE CASCADE,
            content TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_agents_workspace ON workspace_agents(workspace_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_agent_tasks_agent ON workspace_agent_tasks(agent_id, created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_messages_workspace ON workspace_messages(workspace_id, created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_messages_to ON workspace_messages(workspace_id, to_agent_id, created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_logs_workspace ON workspace_logs(workspace_id, created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_pending_events_open ON workspace_pending_events(workspace_id, resolved_at)",
        [],
    )?;

    // 给早于这批列存在时创建的安装做增量迁移。
    let agent_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(workspace_agents)")?
        .query_map([], |row| row.get(1))?
        .filter_map(|r| r.ok())
        .collect();
    if !agent_columns.contains(&"deleted_at".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN deleted_at INTEGER", [])?;
    }
    if !agent_columns.contains(&"rag_top_k".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN rag_top_k INTEGER NOT NULL DEFAULT 5", [])?;
    }
    if !agent_columns.contains(&"rag_retrieval_mode".to_string()) {
        conn.execute(
            "ALTER TABLE workspace_agents ADD COLUMN rag_retrieval_mode TEXT NOT NULL DEFAULT 'hybrid'",
            [],
        )?;
    }
    if !agent_columns.contains(&"scratchpad".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN scratchpad TEXT NOT NULL DEFAULT ''", [])?;
    }
    if !agent_columns.contains(&"rag_reranker_config_id".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN rag_reranker_config_id TEXT", [])?;
    }
    if !agent_columns.contains(&"rag_reranker_base_url".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN rag_reranker_base_url TEXT", [])?;
    }
    if !agent_columns.contains(&"rag_reranker_model".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN rag_reranker_model TEXT", [])?;
    }
    if !agent_columns.contains(&"rag_rerank_top_n".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN rag_rerank_top_n INTEGER", [])?;
    }
    if !agent_columns.contains(&"require_tool_approval".to_string()) {
        conn.execute(
            "ALTER TABLE workspace_agents ADD COLUMN require_tool_approval INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !agent_columns.contains(&"enable_thinking".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN enable_thinking INTEGER NOT NULL DEFAULT 0", [])?;
    }
    if !agent_columns.contains(&"max_tool_rounds".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN max_tool_rounds INTEGER NOT NULL DEFAULT 20", [])?;
    }
    if !agent_columns.contains(&"history_limit".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN history_limit INTEGER NOT NULL DEFAULT 40", [])?;
    }
    if !agent_columns.contains(&"max_tokens".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN max_tokens INTEGER", [])?;
    }
    if !agent_columns.contains(&"tool_whitelist".to_string()) {
        conn.execute("ALTER TABLE workspace_agents ADD COLUMN tool_whitelist TEXT NOT NULL DEFAULT '[]'", [])?;
    }

    log::info!("Workspace SQLite tables initialized");
    Ok(())
}

fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        max_agents: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn insert_workspace(conn: &Connection, ws: &Workspace) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspaces (id, name, description, max_agents, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![ws.id, ws.name, ws.description, ws.max_agents, ws.created_at, ws.updated_at],
    )?;
    Ok(())
}

pub fn list_workspaces(conn: &Connection) -> Result<Vec<Workspace>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, max_agents, created_at, updated_at
         FROM workspaces ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_workspace)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_workspace(conn: &Connection, id: &str) -> Result<Option<Workspace>, WorkspaceError> {
    let result = conn.query_row(
        "SELECT id, name, description, max_agents, created_at, updated_at
         FROM workspaces WHERE id = ?1",
        [id],
        row_to_workspace,
    );
    match result {
        Ok(ws) => Ok(Some(ws)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 删除一个工作组及其下所有内容。手动做级联删除、先删子表再删主表，而不是
/// 单纯依赖上面表定义里的 `ON DELETE CASCADE`——`PRAGMA foreign_keys` 是按
/// 连接（per-connection）生效的设置，而本模块每个业务操作都是新开一个
/// `rusqlite::Connection`，并不复用 `Database::init` 那个长生命周期的连接，
/// 所以级联到底会不会真的触发，本模块不应该指望它。
pub fn delete_workspace(conn: &Connection, id: &str) -> Result<(), WorkspaceError> {
    conn.execute(
        "DELETE FROM workspace_agent_tasks WHERE agent_id IN (SELECT id FROM workspace_agents WHERE workspace_id = ?1)",
        [id],
    )?;
    conn.execute("DELETE FROM workspace_messages WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_logs WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_pending_events WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_agents WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
    Ok(())
}

/// 存活（未软删除）的 Agent 数量，用于 `max_agents` 安全阀检查——已删除的
/// Agent 不该继续占着一个名额。
pub fn count_agents(conn: &Connection, workspace_id: &str) -> Result<i64, WorkspaceError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspace_agents WHERE workspace_id = ?1 AND deleted_at IS NULL",
        [workspace_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

fn row_to_agent(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceAgent> {
    let mcp_server_ids: String = row.get(8)?;
    let knowledge_base_ids: String = row.get(9)?;
    let active_skill_ids: String = row.get(10)?;
    let role: String = row.get(3)?;
    let status: String = row.get(11)?;
    Ok(WorkspaceAgent {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        role: AgentRole::from_str(&role),
        provider: row.get(4)?,
        model: row.get(5)?,
        base_url: row.get(6)?,
        api_config_id: row.get(7)?,
        system_prompt: row.get(12)?,
        mcp_server_ids: serde_json::from_str(&mcp_server_ids).unwrap_or_default(),
        knowledge_base_ids: serde_json::from_str(&knowledge_base_ids).unwrap_or_default(),
        active_skill_ids: serde_json::from_str(&active_skill_ids).unwrap_or_default(),
        status: AgentStatus::from_str(&status),
        rag_top_k: row.get(15)?,
        rag_retrieval_mode: row.get(16)?,
        scratchpad: row.get(17)?,
        deleted_at: row.get(18)?,
        rag_reranker_config_id: row.get(19)?,
        rag_reranker_base_url: row.get(20)?,
        rag_reranker_model: row.get(21)?,
        rag_rerank_top_n: row.get(22)?,
        require_tool_approval: row.get::<_, i64>(23)? != 0,
        enable_thinking: row.get::<_, i64>(24)? != 0,
        max_tool_rounds: row.get(25)?,
        history_limit: row.get(26)?,
        max_tokens: row.get(27)?,
        tool_whitelist: serde_json::from_str(&row.get::<_, String>(28)?).unwrap_or_default(),
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

const AGENT_SELECT_COLUMNS: &str = "id, workspace_id, name, role, provider, model, base_url, api_config_id, \
     mcp_server_ids, knowledge_base_ids, active_skill_ids, status, system_prompt, created_at, updated_at, \
     rag_top_k, rag_retrieval_mode, scratchpad, deleted_at, \
     rag_reranker_config_id, rag_reranker_base_url, rag_reranker_model, rag_rerank_top_n, require_tool_approval, \
     enable_thinking, max_tool_rounds, history_limit, max_tokens, tool_whitelist";

pub fn insert_agent(conn: &Connection, agent: &WorkspaceAgent) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_agents
         (id, workspace_id, name, role, provider, model, base_url, api_config_id,
          system_prompt, mcp_server_ids, knowledge_base_ids, active_skill_ids, status, created_at, updated_at,
          rag_top_k, rag_retrieval_mode, scratchpad, deleted_at,
          rag_reranker_config_id, rag_reranker_base_url, rag_reranker_model, rag_rerank_top_n, require_tool_approval,
          enable_thinking, max_tool_rounds, history_limit, max_tokens, tool_whitelist)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
        rusqlite::params![
            agent.id,
            agent.workspace_id,
            agent.name,
            agent.role.as_str(),
            agent.provider,
            agent.model,
            agent.base_url,
            agent.api_config_id,
            agent.system_prompt,
            serde_json::to_string(&agent.mcp_server_ids).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&agent.knowledge_base_ids).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&agent.active_skill_ids).unwrap_or_else(|_| "[]".to_string()),
            agent.status.as_str(),
            agent.created_at,
            agent.updated_at,
            agent.rag_top_k,
            agent.rag_retrieval_mode,
            agent.scratchpad,
            agent.deleted_at,
            agent.rag_reranker_config_id,
            agent.rag_reranker_base_url,
            agent.rag_reranker_model,
            agent.rag_rerank_top_n,
            agent.require_tool_approval as i64,
            agent.enable_thinking as i64,
            agent.max_tool_rounds,
            agent.history_limit,
            agent.max_tokens,
            serde_json::to_string(&agent.tool_whitelist).unwrap_or_else(|_| "[]".to_string()),
        ],
    )?;
    Ok(())
}

/// 按 id 查一个 Agent，不管它是否已被软删除——既用于运行中的 Agent 循环
/// （反正它手上拿到的 id 本来就都是存活 Agent 的），也用于给已被删除的
/// Agent 发出的历史消息/日志回填显示名字。
pub fn get_agent(conn: &Connection, id: &str) -> Result<Option<WorkspaceAgent>, WorkspaceError> {
    let sql = format!("SELECT {} FROM workspace_agents WHERE id = ?1", AGENT_SELECT_COLUMNS);
    let result = conn.query_row(&sql, [id], row_to_agent);
    match result {
        Ok(agent) => Ok(Some(agent)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 只返回存活的 Agent（排除软删除的）——这也是现有每个调用方（Agent 名册、
/// 会议与会者、max-agents 计数、主 Agent 查找）实际想要的结果。如果需要
/// 已删除 Agent 的行仍然可查（比如历史消息的发送者名字），直接用 `get_agent`。
pub fn list_agents(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceAgent>, WorkspaceError> {
    let sql = format!(
        "SELECT {} FROM workspace_agents WHERE workspace_id = ?1 AND deleted_at IS NULL ORDER BY created_at ASC",
        AGENT_SELECT_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([workspace_id], row_to_agent)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 包含软删除在内的全部 Agent——只在需要解析已删除 Agent 名字的场景使用
/// （渲染引用了它的历史消息/日志）。其余场景一律用 `list_agents`。
pub fn list_agents_including_deleted(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceAgent>, WorkspaceError> {
    let sql = format!(
        "SELECT {} FROM workspace_agents WHERE workspace_id = ?1 ORDER BY created_at ASC",
        AGENT_SELECT_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([workspace_id], row_to_agent)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn update_agent_status(conn: &Connection, id: &str, status: AgentStatus) -> Result<(), WorkspaceError> {
    conn.execute(
        "UPDATE workspace_agents SET status = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![status.as_str(), chrono::Utc::now().timestamp_millis(), id],
    )?;
    Ok(())
}

/// 把用户的编辑应用到一个已存在 Agent 的配置上。刻意不改动 `status`——
/// 运行中的循环每次唤醒都会重新读一遍这一行，所以普通字段更新在 Agent 下一轮
/// 就会生效，不需要重启它的后台任务。
pub fn update_agent(conn: &Connection, req: &UpdateAgentRequest) -> Result<(), WorkspaceError> {
    let rows = conn.execute(
        "UPDATE workspace_agents SET name = ?1, provider = ?2, model = ?3, base_url = ?4, api_config_id = ?5, \
         system_prompt = ?6, mcp_server_ids = ?7, knowledge_base_ids = ?8, active_skill_ids = ?9, \
         rag_top_k = ?10, rag_retrieval_mode = ?11, \
         rag_reranker_config_id = ?12, rag_reranker_base_url = ?13, rag_reranker_model = ?14, rag_rerank_top_n = ?15, \
         require_tool_approval = ?16, enable_thinking = ?17, max_tool_rounds = ?18, history_limit = ?19, \
         max_tokens = ?20, tool_whitelist = ?21, updated_at = ?22 \
         WHERE id = ?23 AND deleted_at IS NULL",
        rusqlite::params![
            req.name,
            req.provider,
            req.model,
            req.base_url,
            req.api_config_id,
            req.system_prompt,
            serde_json::to_string(&req.mcp_server_ids).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&req.knowledge_base_ids).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&req.active_skill_ids).unwrap_or_else(|_| "[]".to_string()),
            req.rag_top_k,
            req.rag_retrieval_mode,
            req.rag_reranker_config_id,
            req.rag_reranker_base_url,
            req.rag_reranker_model,
            req.rag_rerank_top_n,
            req.require_tool_approval as i64,
            req.enable_thinking as i64,
            req.max_tool_rounds.max(1),
            req.history_limit.max(1),
            req.max_tokens,
            serde_json::to_string(&req.tool_whitelist).unwrap_or_else(|_| "[]".to_string()),
            chrono::Utc::now().timestamp_millis(),
            req.id,
        ],
    )?;
    if rows == 0 {
        return Err(WorkspaceError::AgentNotFound(req.id.clone()));
    }
    Ok(())
}

pub fn get_scratchpad(conn: &Connection, agent_id: &str) -> Result<String, WorkspaceError> {
    let result = conn.query_row(
        "SELECT scratchpad FROM workspace_agents WHERE id = ?1",
        [agent_id],
        |row| row.get(0),
    );
    match result {
        Ok(s) => Ok(s),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn set_scratchpad(conn: &Connection, agent_id: &str, content: &str) -> Result<(), WorkspaceError> {
    conn.execute(
        "UPDATE workspace_agents SET scratchpad = ?1 WHERE id = ?2",
        rusqlite::params![content, agent_id],
    )?;
    Ok(())
}

/// 把一个工具加入 Agent 的审批白名单（幂等：已在名单里就不重复加）。
/// 由审批卡片的"记住选择"触发；撤销走 `update_agent`（编辑表单整体保存）。
pub fn add_tool_to_whitelist(conn: &Connection, agent_id: &str, tool_name: &str) -> Result<(), WorkspaceError> {
    let current: String = conn.query_row(
        "SELECT tool_whitelist FROM workspace_agents WHERE id = ?1",
        [agent_id],
        |row| row.get(0),
    )?;
    let mut list: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    if list.iter().any(|t| t == tool_name) {
        return Ok(());
    }
    list.push(tool_name.to_string());
    conn.execute(
        "UPDATE workspace_agents SET tool_whitelist = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
            chrono::Utc::now().timestamp_millis(),
            agent_id
        ],
    )?;
    Ok(())
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceAgentTask> {
    Ok(WorkspaceAgentTask {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        content: row.get(2)?,
        done: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn insert_task(conn: &Connection, task: &WorkspaceAgentTask) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_agent_tasks (id, agent_id, content, done, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![task.id, task.agent_id, task.content, task.done as i64, task.created_at, task.updated_at],
    )?;
    Ok(())
}

/// 一个 Agent 的全部任务，最早的在前——未完成项自然排在后完成的前面，
/// 因为 `done` 字段不会导致这一行重新排序。
pub fn list_tasks(conn: &Connection, agent_id: &str) -> Result<Vec<WorkspaceAgentTask>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, content, done, created_at, updated_at
         FROM workspace_agent_tasks WHERE agent_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([agent_id], row_to_task)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_task_done(conn: &Connection, task_id: &str, done: bool) -> Result<(), WorkspaceError> {
    let rows = conn.execute(
        "UPDATE workspace_agent_tasks SET done = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![done as i64, chrono::Utc::now().timestamp_millis(), task_id],
    )?;
    if rows == 0 {
        return Err(WorkspaceError::NotFound(format!("任务 {} 不存在", task_id)));
    }
    Ok(())
}

pub fn delete_task(conn: &Connection, task_id: &str) -> Result<(), WorkspaceError> {
    conn.execute("DELETE FROM workspace_agent_tasks WHERE id = ?1", [task_id])?;
    Ok(())
}

/// 软删除：保留这一行（设置 `deleted_at`），这样引用这个 agent id 的历史
/// 消息/日志仍能解析出名字，而不是在时间线上退化成显示一串原始 UUID。
pub fn delete_agent(conn: &Connection, id: &str) -> Result<(), WorkspaceError> {
    conn.execute(
        "UPDATE workspace_agents SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![chrono::Utc::now().timestamp_millis(), id],
    )?;
    Ok(())
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceMessage> {
    Ok(WorkspaceMessage {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        from_agent_id: row.get(2)?,
        to_agent_id: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub fn insert_message(conn: &Connection, msg: &WorkspaceMessage) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_messages (id, workspace_id, from_agent_id, to_agent_id, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![msg.id, msg.workspace_id, msg.from_agent_id, msg.to_agent_id, msg.content, msg.created_at],
    )?;
    Ok(())
}

/// 一个工作组的全部消息，最早的在前——供前端渲染完整时间线用。
pub fn list_messages(conn: &Connection, workspace_id: &str, limit: i64) -> Result<Vec<WorkspaceMessage>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, from_agent_id, to_agent_id, content, created_at
         FROM workspace_messages WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![workspace_id, limit], row_to_message)?;
    let mut messages: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    messages.reverse();
    Ok(messages)
}

/// 跟某个 Agent 自身上下文相关的近期消息：直接发给它的、广播给 "all" 的，
/// 或者它之前自己发过的。最早的在前，用 `limit` 封顶，避免一个长期运行的
/// 工作组把每轮提示词无限撑大。
pub fn list_recent_messages_for_agent(
    conn: &Connection,
    workspace_id: &str,
    agent_id: &str,
    limit: i64,
) -> Result<Vec<WorkspaceMessage>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, from_agent_id, to_agent_id, content, created_at
         FROM workspace_messages
         WHERE workspace_id = ?1 AND (to_agent_id = ?2 OR to_agent_id = 'all' OR from_agent_id = ?2)
         ORDER BY created_at DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![workspace_id, agent_id, limit], row_to_message)?;
    let mut messages: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    messages.reverse();
    Ok(messages)
}

fn row_to_log(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceLogEntry> {
    Ok(WorkspaceLogEntry {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        agent_id: row.get(2)?,
        kind: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub fn insert_log(conn: &Connection, entry: &WorkspaceLogEntry) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_logs (id, workspace_id, agent_id, kind, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![entry.id, entry.workspace_id, entry.agent_id, entry.kind, entry.content, entry.created_at],
    )?;
    Ok(())
}

pub fn list_logs(conn: &Connection, workspace_id: &str, limit: i64) -> Result<Vec<WorkspaceLogEntry>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, agent_id, kind, content, created_at
         FROM workspace_logs WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![workspace_id, limit], row_to_log)?;
    let mut logs: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    logs.reverse();
    Ok(logs)
}

fn row_to_pending_event(row: &rusqlite::Row) -> rusqlite::Result<WorkspacePendingEvent> {
    let payload: String = row.get(5)?;
    Ok(WorkspacePendingEvent {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        agent_id: row.get(2)?,
        agent_name: row.get(3)?,
        kind: row.get(4)?,
        payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
        created_at: row.get(6)?,
        resolved_at: row.get(7)?,
    })
}

/// 在一个提议/休眠请求/问题被提出的那一刻就记录下来，这样即便应用重启、
/// 或者一次性前端事件触发时用户根本没在看这个页面，它也不会丢。`id` 必须
/// 跟用来解决它的那个 id（`proposal_id`/`request_id`/`question_id`）一致，
/// 这样 `resolve_pending_event` 才能重新找到它。
pub fn insert_pending_event(conn: &Connection, event: &WorkspacePendingEvent) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_pending_events (id, workspace_id, agent_id, agent_name, kind, payload, created_at, resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
        rusqlite::params![
            event.id,
            event.workspace_id,
            event.agent_id,
            event.agent_name,
            event.kind,
            serde_json::to_string(&event.payload).unwrap_or_else(|_| "null".to_string()),
            event.created_at,
        ],
    )?;
    Ok(())
}

pub fn resolve_pending_event(conn: &Connection, id: &str) -> Result<(), WorkspaceError> {
    conn.execute(
        "UPDATE workspace_pending_events SET resolved_at = ?1 WHERE id = ?2",
        rusqlite::params![chrono::Utc::now().timestamp_millis(), id],
    )?;
    Ok(())
}

/// 这个工作组里所有还在等人工决策的事项，最早的在前——前端在选中一个
/// 工作组时会拉这个接口，把页面（或整个应用）没打开期间错过的内容补回来。
pub fn list_unresolved_pending_events(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspacePendingEvent>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, agent_id, agent_name, kind, payload, created_at, resolved_at
         FROM workspace_pending_events WHERE workspace_id = ?1 AND resolved_at IS NULL ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([workspace_id], row_to_pending_event)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 把**所有**未解决的待处理事项统一标记为已解决（过期），返回每个工作组
/// 被过期的条数。只该在应用启动时调用：这些事项对应的等待通道（oneshot）
/// 已随上一个进程消亡，永远不可能再被批准或拒绝——不清掉它们，前端每次
/// 选中工作组都会把它们拉出来，变成一批点了就报错、又永远不消失的僵尸卡片。
pub fn expire_unresolved_pending_events(conn: &Connection) -> Result<Vec<(String, i64)>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, COUNT(*) FROM workspace_pending_events WHERE resolved_at IS NULL GROUP BY workspace_id",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    let expired: Vec<(String, i64)> = rows.filter_map(|r| r.ok()).collect();
    if !expired.is_empty() {
        conn.execute(
            "UPDATE workspace_pending_events SET resolved_at = ?1 WHERE resolved_at IS NULL",
            rusqlite::params![chrono::Utc::now().timestamp_millis()],
        )?;
    }
    Ok(expired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_workspace_tables(&conn).unwrap();
        conn
    }

    fn sample_agent(workspace_id: &str, role: AgentRole, name: &str) -> WorkspaceAgent {
        WorkspaceAgent {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            name: name.to_string(),
            role,
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            base_url: String::new(),
            api_config_id: "cfg-1".to_string(),
            system_prompt: "be helpful".to_string(),
            mcp_server_ids: vec!["mcp-1".to_string()],
            knowledge_base_ids: vec![],
            active_skill_ids: vec![],
            status: AgentStatus::Idle,
            rag_top_k: 5,
            rag_retrieval_mode: "hybrid".to_string(),
            rag_reranker_config_id: None,
            rag_reranker_base_url: None,
            rag_reranker_model: None,
            rag_rerank_top_n: None,
            scratchpad: String::new(),
            require_tool_approval: true,
            enable_thinking: false,
            // 刻意用非默认值，任何列错位/漏写都会让往返断言直接失败
            max_tool_rounds: 12,
            history_limit: 25,
            max_tokens: Some(4096),
            tool_whitelist: vec!["write_file".to_string()],
            deleted_at: None,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[test]
    fn workspace_round_trips_through_insert_get_list() {
        let conn = setup();
        let ws = Workspace {
            id: "ws-1".to_string(),
            name: "Test WS".to_string(),
            description: "desc".to_string(),
            max_agents: 3,
            created_at: 100,
            updated_at: 100,
        };
        insert_workspace(&conn, &ws).unwrap();

        let fetched = get_workspace(&conn, "ws-1").unwrap().expect("workspace should exist");
        assert_eq!(fetched.name, "Test WS");
        assert_eq!(fetched.max_agents, 3);

        let all = list_workspaces(&conn).unwrap();
        assert_eq!(all.len(), 1);

        assert!(get_workspace(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn agent_round_trip_preserves_role_and_json_id_lists() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();

        let agent = sample_agent("ws-1", AgentRole::Main, "Main");
        insert_agent(&conn, &agent).unwrap();

        let fetched = get_agent(&conn, &agent.id).unwrap().expect("agent should exist");
        assert_eq!(fetched.role, AgentRole::Main);
        assert_eq!(fetched.mcp_server_ids, vec!["mcp-1".to_string()]);
        assert_eq!(fetched.status, AgentStatus::Idle);
        assert_eq!(fetched.system_prompt, "be helpful");
    }

    #[test]
    fn count_agents_tracks_inserts_for_the_max_agents_safety_cap() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 2, created_at: 0, updated_at: 0 },
        )
        .unwrap();

        assert_eq!(count_agents(&conn, "ws-1").unwrap(), 0);
        insert_agent(&conn, &sample_agent("ws-1", AgentRole::Main, "A")).unwrap();
        insert_agent(&conn, &sample_agent("ws-1", AgentRole::Sub, "B")).unwrap();
        assert_eq!(count_agents(&conn, "ws-1").unwrap(), 2);
    }

    #[test]
    fn update_agent_status_persists_new_status() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();

        update_agent_status(&conn, &agent.id, AgentStatus::Running).unwrap();
        let fetched = get_agent(&conn, &agent.id).unwrap().unwrap();
        assert_eq!(fetched.status, AgentStatus::Running);
    }

    #[test]
    fn list_recent_messages_for_agent_includes_direct_broadcast_and_own_sent_messages() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let msgs = [
            ("user", "agent-a", "hello A"),
            ("agent-a", "user", "hi back"),
            ("agent-b", "all", "broadcast to everyone"),
            ("agent-b", "agent-c", "not relevant to A"),
        ];
        for (i, (from, to, content)) in msgs.iter().enumerate() {
            insert_message(
                &conn,
                &WorkspaceMessage {
                    id: format!("m{}", i),
                    workspace_id: "ws-1".to_string(),
                    from_agent_id: from.to_string(),
                    to_agent_id: to.to_string(),
                    content: content.to_string(),
                    created_at: i as i64,
                },
            )
            .unwrap();
        }

        let relevant = list_recent_messages_for_agent(&conn, "ws-1", "agent-a", 10).unwrap();
        let contents: Vec<_> = relevant.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, vec!["hello A", "hi back", "broadcast to everyone"]);
    }

    #[test]
    fn delete_workspace_removes_agents_messages_and_logs_without_relying_on_fk_cascade() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Main, "A");
        insert_agent(&conn, &agent).unwrap();
        insert_message(
            &conn,
            &WorkspaceMessage { id: "m1".into(), workspace_id: "ws-1".into(), from_agent_id: "user".into(), to_agent_id: agent.id.clone(), content: "hi".into(), created_at: 0 },
        )
        .unwrap();
        insert_log(
            &conn,
            &WorkspaceLogEntry { id: "l1".into(), workspace_id: "ws-1".into(), agent_id: Some(agent.id.clone()), kind: "agent_created".into(), content: "created".into(), created_at: 0 },
        )
        .unwrap();

        delete_workspace(&conn, "ws-1").unwrap();

        assert!(get_workspace(&conn, "ws-1").unwrap().is_none());
        assert!(list_agents(&conn, "ws-1").unwrap().is_empty());
        assert!(list_messages(&conn, "ws-1", 10).unwrap().is_empty());
        assert!(list_logs(&conn, "ws-1", 10).unwrap().is_empty());
    }

    #[test]
    fn deleted_agent_is_excluded_from_list_and_count_but_still_resolvable_by_id() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();
        assert_eq!(count_agents(&conn, "ws-1").unwrap(), 1);

        delete_agent(&conn, &agent.id).unwrap();

        assert_eq!(count_agents(&conn, "ws-1").unwrap(), 0);
        assert!(list_agents(&conn, "ws-1").unwrap().is_empty());
        // 仍然能按 id 查到——历史消息/日志需要这个名字。
        let fetched = get_agent(&conn, &agent.id).unwrap().expect("soft-deleted agent should still be gettable by id");
        assert!(fetched.deleted_at.is_some());
        assert_eq!(fetched.name, "A");
    }

    #[test]
    fn update_agent_persists_edits_without_touching_status() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();
        update_agent_status(&conn, &agent.id, AgentStatus::Running).unwrap();

        update_agent(
            &conn,
            &UpdateAgentRequest {
                id: agent.id.clone(),
                name: "A-renamed".to_string(),
                provider: "deepseek".to_string(),
                model: "deepseek-v4".to_string(),
                base_url: "https://api.deepseek.com/v1".to_string(),
                api_config_id: "cfg-2".to_string(),
                system_prompt: "be terse".to_string(),
                mcp_server_ids: vec![],
                knowledge_base_ids: vec!["kb-1".to_string()],
                active_skill_ids: vec![],
                rag_top_k: 8,
                rag_retrieval_mode: "vector".to_string(),
                rag_reranker_config_id: Some("rerank-cfg-1".to_string()),
                rag_reranker_base_url: Some("https://api.cohere.com".to_string()),
                rag_reranker_model: Some("rerank-multilingual-v3.0".to_string()),
                rag_rerank_top_n: Some(3),
                require_tool_approval: false,
                enable_thinking: true,
                max_tool_rounds: 33,
                history_limit: 60,
                max_tokens: Some(9000),
                tool_whitelist: vec!["exec_cmd".to_string()],
            },
        )
        .unwrap();

        let fetched = get_agent(&conn, &agent.id).unwrap().unwrap();
        assert_eq!(fetched.name, "A-renamed");
        assert_eq!(fetched.max_tool_rounds, 33);
        assert_eq!(fetched.history_limit, 60);
        assert_eq!(fetched.max_tokens, Some(9000));
        assert_eq!(fetched.tool_whitelist, vec!["exec_cmd".to_string()]);
        assert_eq!(fetched.provider, "deepseek");
        assert_eq!(fetched.rag_top_k, 8);
        assert_eq!(fetched.rag_retrieval_mode, "vector");
        assert_eq!(fetched.knowledge_base_ids, vec!["kb-1".to_string()]);
        assert_eq!(fetched.rag_reranker_config_id, Some("rerank-cfg-1".to_string()));
        assert_eq!(fetched.rag_rerank_top_n, Some(3));
        assert!(!fetched.require_tool_approval);
        assert!(fetched.enable_thinking);
        // 编辑不会改动状态——运行中的循环重新加载的是配置，不是状态。
        assert_eq!(fetched.status, AgentStatus::Running);

        assert!(matches!(
            update_agent(
                &conn,
                &UpdateAgentRequest {
                    id: "missing".to_string(),
                    name: "x".to_string(),
                    provider: "x".to_string(),
                    model: "x".to_string(),
                    base_url: "".to_string(),
                    api_config_id: "".to_string(),
                    system_prompt: "".to_string(),
                    mcp_server_ids: vec![],
                    knowledge_base_ids: vec![],
                    active_skill_ids: vec![],
                    rag_top_k: 5,
                    rag_retrieval_mode: "hybrid".to_string(),
                    rag_reranker_config_id: None,
                    rag_reranker_base_url: None,
                    rag_reranker_model: None,
                    rag_rerank_top_n: None,
                    require_tool_approval: true,
                    enable_thinking: false,
                    max_tool_rounds: 20,
                    history_limit: 40,
                    max_tokens: None,
                    tool_whitelist: vec![],
                },
            ),
            Err(WorkspaceError::AgentNotFound(_))
        ));
    }

    #[test]
    fn scratchpad_round_trips_and_defaults_to_empty() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();

        assert_eq!(get_scratchpad(&conn, &agent.id).unwrap(), "");
        set_scratchpad(&conn, &agent.id, "已联系客户，等回复").unwrap();
        assert_eq!(get_scratchpad(&conn, &agent.id).unwrap(), "已联系客户，等回复");
    }

    #[test]
    fn pending_events_round_trip_and_resolve_excludes_from_unresolved_list() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();

        let event = WorkspacePendingEvent {
            id: "evt-1".to_string(),
            workspace_id: "ws-1".to_string(),
            agent_id: "agent-a".to_string(),
            agent_name: "A".to_string(),
            kind: "sleep".to_string(),
            payload: serde_json::json!({ "reason": "done for now" }),
            created_at: 100,
            resolved_at: None,
        };
        insert_pending_event(&conn, &event).unwrap();

        let unresolved = list_unresolved_pending_events(&conn, "ws-1").unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].payload["reason"], "done for now");

        resolve_pending_event(&conn, "evt-1").unwrap();
        assert!(list_unresolved_pending_events(&conn, "ws-1").unwrap().is_empty());
    }

    #[test]
    fn tasks_round_trip_ordered_and_toggle_done() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();

        insert_task(&conn, &WorkspaceAgentTask { id: "t1".into(), agent_id: agent.id.clone(), content: "查资料".into(), done: false, created_at: 1, updated_at: 1 }).unwrap();
        insert_task(&conn, &WorkspaceAgentTask { id: "t2".into(), agent_id: agent.id.clone(), content: "写总结".into(), done: false, created_at: 2, updated_at: 2 }).unwrap();

        let tasks = list_tasks(&conn, &agent.id).unwrap();
        assert_eq!(tasks.iter().map(|t| t.content.as_str()).collect::<Vec<_>>(), vec!["查资料", "写总结"]);
        assert!(tasks.iter().all(|t| !t.done));

        set_task_done(&conn, "t1", true).unwrap();
        let tasks = list_tasks(&conn, &agent.id).unwrap();
        assert!(tasks[0].done);
        assert!(!tasks[1].done);

        delete_task(&conn, "t2").unwrap();
        assert_eq!(list_tasks(&conn, &agent.id).unwrap().len(), 1);

        assert!(matches!(set_task_done(&conn, "missing", true), Err(WorkspaceError::NotFound(_))));
    }

    #[test]
    fn delete_workspace_also_removes_agent_tasks() {
        let conn = setup();
        insert_workspace(
            &conn,
            &Workspace { id: "ws-1".into(), name: "WS".into(), description: "".into(), max_agents: 5, created_at: 0, updated_at: 0 },
        )
        .unwrap();
        let agent = sample_agent("ws-1", AgentRole::Sub, "A");
        insert_agent(&conn, &agent).unwrap();
        insert_task(&conn, &WorkspaceAgentTask { id: "t1".into(), agent_id: agent.id.clone(), content: "任务".into(), done: false, created_at: 1, updated_at: 1 }).unwrap();

        delete_workspace(&conn, "ws-1").unwrap();

        assert!(list_tasks(&conn, &agent.id).unwrap().is_empty());
    }
}
