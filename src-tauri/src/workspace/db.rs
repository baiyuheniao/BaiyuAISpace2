// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use rusqlite::Connection;

/// Opens a connection with a busy-timeout set, so a write that lands while
/// another connection briefly holds the write lock retries for up to 5s
/// instead of failing immediately with `SQLITE_BUSY` -- every business
/// operation in this module opens its own short-lived connection (see
/// `delete_workspace`'s doc comment), so concurrent writes from two agents'
/// loops firing at once is a real, not theoretical, scenario.
pub fn open_conn(path: &str) -> Result<Connection, WorkspaceError> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// Create the Workspace tables if they don't already exist, and additively
/// migrate older installs (`ALTER TABLE ... ADD COLUMN`, guarded by checking
/// `PRAGMA table_info` first) up to the current schema.
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
        "CREATE INDEX IF NOT EXISTS idx_workspace_agents_workspace ON workspace_agents(workspace_id)",
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

    // Additive migrations for installs created before this column set existed.
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

/// Deletes a workspace and everything in it. Does the cascade manually,
/// children first, rather than relying solely on the `ON DELETE CASCADE` in
/// the table defs above -- `PRAGMA foreign_keys` is a per-connection setting
/// and every business operation here opens a fresh `rusqlite::Connection`
/// rather than reusing `Database::init`'s long-lived one, so whether cascade
/// actually fires isn't something this module should depend on.
pub fn delete_workspace(conn: &Connection, id: &str) -> Result<(), WorkspaceError> {
    conn.execute("DELETE FROM workspace_messages WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_logs WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_pending_events WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspace_agents WHERE workspace_id = ?1", [id])?;
    conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
    Ok(())
}

/// Active (non-soft-deleted) agent count, used for the `max_agents` safety
/// cap -- a deleted agent shouldn't hold a seat open.
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
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

const AGENT_SELECT_COLUMNS: &str = "id, workspace_id, name, role, provider, model, base_url, api_config_id, \
     mcp_server_ids, knowledge_base_ids, active_skill_ids, status, system_prompt, created_at, updated_at, \
     rag_top_k, rag_retrieval_mode, scratchpad, deleted_at";

pub fn insert_agent(conn: &Connection, agent: &WorkspaceAgent) -> Result<(), WorkspaceError> {
    conn.execute(
        "INSERT INTO workspace_agents
         (id, workspace_id, name, role, provider, model, base_url, api_config_id,
          system_prompt, mcp_server_ids, knowledge_base_ids, active_skill_ids, status, created_at, updated_at,
          rag_top_k, rag_retrieval_mode, scratchpad, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
        ],
    )?;
    Ok(())
}

/// Looks up an agent by id regardless of soft-delete status -- used both for
/// the live agent-loop path (which only ever holds ids of active agents
/// anyway) and to backfill display names for historical messages/logs sent
/// by an agent that has since been deleted.
pub fn get_agent(conn: &Connection, id: &str) -> Result<Option<WorkspaceAgent>, WorkspaceError> {
    let sql = format!("SELECT {} FROM workspace_agents WHERE id = ?1", AGENT_SELECT_COLUMNS);
    let result = conn.query_row(&sql, [id], row_to_agent);
    match result {
        Ok(agent) => Ok(Some(agent)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Active agents only (excludes soft-deleted) -- what every existing caller
/// (agent roster, meeting participants, max-agents count, main-agent lookup)
/// actually wants. Use `get_agent` directly when a deleted agent's row still
/// needs to be resolvable (e.g. historical message sender names).
pub fn list_agents(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceAgent>, WorkspaceError> {
    let sql = format!(
        "SELECT {} FROM workspace_agents WHERE workspace_id = ?1 AND deleted_at IS NULL ORDER BY created_at ASC",
        AGENT_SELECT_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([workspace_id], row_to_agent)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// All agents including soft-deleted ones -- used only where a deleted
/// agent's name still needs to resolve (rendering historical messages/logs
/// that reference it). Everything else should use `list_agents`.
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

/// Applies user edits to an existing agent's config. Deliberately doesn't
/// touch `status` -- the live loop reloads the row fresh every wake, so a
/// plain field update takes effect on the agent's next turn without needing
/// to restart its background task.
pub fn update_agent(conn: &Connection, req: &UpdateAgentRequest) -> Result<(), WorkspaceError> {
    let rows = conn.execute(
        "UPDATE workspace_agents SET name = ?1, provider = ?2, model = ?3, base_url = ?4, api_config_id = ?5, \
         system_prompt = ?6, mcp_server_ids = ?7, knowledge_base_ids = ?8, active_skill_ids = ?9, \
         rag_top_k = ?10, rag_retrieval_mode = ?11, updated_at = ?12 \
         WHERE id = ?13 AND deleted_at IS NULL",
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

/// Soft delete: keeps the row (with `deleted_at` set) so historical
/// messages/logs referencing this agent id can still resolve its name,
/// instead of falling back to a raw UUID in the timeline.
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

/// All messages in a workspace, oldest first -- used by the frontend to
/// render the full timeline.
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

/// Recent messages relevant to one agent's own context: anything addressed
/// to it directly, broadcast to "all", or sent by it previously. Oldest
/// first, capped at `limit` so a long-running workspace doesn't grow the
/// per-turn prompt unbounded.
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

/// Records a proposal/sleep-request/question the moment it's raised, so it
/// survives a restart or a user who wasn't looking at this page when the
/// one-shot frontend event fired. `id` must match the id used to resolve it
/// (`proposal_id`/`request_id`/`question_id`) so `resolve_pending_event` can
/// find it again.
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

/// Everything still awaiting a human decision in this workspace, oldest
/// first -- what the frontend fetches on selecting a workspace to backfill
/// anything it missed while the page (or the app) wasn't open.
pub fn list_unresolved_pending_events(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspacePendingEvent>, WorkspaceError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, agent_id, agent_name, kind, payload, created_at, resolved_at
         FROM workspace_pending_events WHERE workspace_id = ?1 AND resolved_at IS NULL ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([workspace_id], row_to_pending_event)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
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
            scratchpad: String::new(),
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
        // Still resolvable by id -- historical messages/logs need the name.
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
            },
        )
        .unwrap();

        let fetched = get_agent(&conn, &agent.id).unwrap().unwrap();
        assert_eq!(fetched.name, "A-renamed");
        assert_eq!(fetched.provider, "deepseek");
        assert_eq!(fetched.rag_top_k, 8);
        assert_eq!(fetched.rag_retrieval_mode, "vector");
        assert_eq!(fetched.knowledge_base_ids, vec!["kb-1".to_string()]);
        // Status untouched by an edit -- the live loop reloads config, not status.
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
}
