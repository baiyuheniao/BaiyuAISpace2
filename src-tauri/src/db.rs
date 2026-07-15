// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 数据库模块
 * 
 * 功能说明:
 * - SQLite 数据库初始化和表结构创建
 * - 会话 (Session) 管理 (CRUD)
 * - 消息 (Message) 管理
 * - MCP 服务器配置存储
 * - 数据库迁移支持
 * 
 * 数据库表:
 * - sessions: 聊天会话表
 * - messages: 消息表 (关联 sessions)
 * - mcp_servers: MCP 服务器配置表
 */

use crate::types::{ChatMessage, ChatSession, MCPServer, MCPServerType, Skill};
use keyring::Entry;
use std::sync::Arc;
use tauri::Manager;

const MCP_KEYRING_SERVICE: &str = "mcp_api_key";

pub struct Database {
    pub path: String,
    pub conn: rusqlite::Connection,
}

impl Database {
    /**
     * 创建新的数据库实例
     * 
     * @param app_handle: Tauri 应用句柄
     * @return Database 实例
     */
    pub fn new(app_handle: &tauri::AppHandle) -> Self {
        let app_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir");
        std::fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        let db_path = app_dir.join("app.db");
        
        let conn = rusqlite::Connection::open(&db_path).expect("Failed to open database");
        
        Self {
            path: db_path.to_str().unwrap().to_string(),
            conn,
        }
    }

    /**
     * 初始化数据库表结构
     * 
     * 创建以下表:
     * - sessions: 聊天会话
     * - messages: 消息 (外键关联 sessions)
     * - mcp_servers: MCP 服务器配置
     * 
     * 同时创建索引以优化查询性能
     */
    pub fn init(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                api_config_id TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        let has_column = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'api_config_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
        if !has_column {
            self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN api_config_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            log::info!("Database migration: added api_config_id column");
        }

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                error TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
            [],
        )?;

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                server_type TEXT NOT NULL,
                command TEXT,
                args TEXT,
                env TEXT,
                port INTEGER,
                url TEXT,
                api_key TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                instructions TEXT NOT NULL DEFAULT '',
                bound_mcp_server_ids TEXT NOT NULL DEFAULT '[]',
                enabled BOOLEAN NOT NULL DEFAULT 1,
                resource_files TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_skills_enabled ON skills(enabled)",
            [],
        )?;

        log::info!("Database initialized at: {}", self.path);
        Ok(())
    }

    /**
     * 保存会话到数据库
     * 
     * @param session: 要保存的会话对象
     */
    pub fn save_session(&self, session: &ChatSession) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            r#"
            INSERT INTO sessions (id, title, provider, model, api_config_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                provider = excluded.provider,
                model = excluded.model,
                api_config_id = excluded.api_config_id,
                updated_at = excluded.updated_at
            "#,
            [
                &session.id,
                &session.title,
                &session.provider,
                &session.model,
                &session.api_config_id,
                &session.created_at.to_string(),
                &session.updated_at.to_string(),
            ],
        )?;

        log::info!("Session saved: {}", session.id);
        Ok(())
    }

    /**
     * 删除会话
     * 
     * @param session_id: 要删除的会话 ID
     */
    pub fn delete_session(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            [session_id],
        )?;

        log::info!("Session deleted: {}", session_id);
        Ok(())
    }

    /**
     * 获取所有会话
     * 按最后更新时间倒序排列
     * 
     * @return 会话列表 (包含消息)
     */
    pub fn get_sessions(&self) -> Result<Vec<ChatSession>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, title, provider, model, api_config_id, created_at, updated_at 
            FROM sessions 
            ORDER BY updated_at DESC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, title, provider, model, api_config_id, created_at, updated_at) = row?;
            let messages = self.get_messages(&id)?;
            
            sessions.push(ChatSession {
                id,
                title,
                provider,
                model,
                api_config_id,
                created_at,
                updated_at,
                messages,
            });
        }

        log::info!("Loaded {} sessions", sessions.len());
        for s in &sessions {
            log::debug!("Session {} has {} messages", s.id, s.messages.len());
        }
        Ok(sessions)
    }

    /**
     * 保存消息到数据库
     * 同时更新会话的 updated_at 时间戳
     * 
     * @param session_id: 所属会话 ID
     * @param message: 要保存的消息
     */
    pub fn save_message(
        &self,
        session_id: &str,
        message: &ChatMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            r#"
            INSERT INTO messages (id, session_id, role, content, timestamp, error)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                content = excluded.content,
                error = excluded.error
            "#,
            [
                &message.id,
                session_id,
                &message.role,
                &message.content,
                &message.timestamp.to_string(),
                &message.error.as_deref().unwrap_or(""),
            ],
        )?;

        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            [&chrono::Utc::now().timestamp_millis().to_string(), session_id],
        )?;

        log::info!("[save_message] saved message {} for session {}", message.id, session_id);
        Ok(())
    }

    /**
     * 获取指定会话的所有消息
     * 按时间戳升序排列
     * 
     * @param session_id: 会话 ID
     * @return 消息列表
     */
    pub fn get_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
        log::info!("[get_messages] Querying messages for session_id: {}, db_path: {}", session_id, self.path);
        
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, role, content, timestamp, error 
            FROM messages 
            WHERE session_id = ? 
            ORDER BY timestamp ASC
            "#,
        )?;
        
        let rows = stmt.query_map([session_id], |row| {
            let error: Option<String> = row.get(4)?;
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                timestamp: row.get(3)?,
                error: if error.as_deref() == Some("") { None } else { error },
                images: vec![],
                videos: vec![],
            })
        })?;

        let messages: Result<Vec<_>, _> = rows.collect();
        log::info!("get_messages for session {}: {} messages", session_id, messages.as_ref().map(|m| m.len()).unwrap_or(0));
        Ok(messages?)
    }

    /**
     * 保存 MCP 服务器配置
     * 
     * @param server: MCP 服务器配置
     */
    pub fn save_mcp_server(&self, server: &MCPServer) -> Result<(), Box<dyn std::error::Error>> {
        let args_json = serde_json::to_string(&server.args)?;
        let env_json = serde_json::to_string(&server.env)?;
        let server_type = match server.server_type {
            MCPServerType::Stdio => "stdio",
            MCPServerType::SSE => "sse",
            MCPServerType::HTTP => "http",
        }
        .to_string();
        
        let now = chrono::Utc::now().timestamp_millis();
        let port = server.port.map(|p| p as i64);
        
        if let Some(ref api_key) = server.api_key {
            if !api_key.is_empty() {
                let entry = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server.id, "api_key"))
                    .map_err(|e| { log::error!("创建密钥链条目失败（详情：{}）", e); "保存 API 密钥失败，请检查系统密钥链权限".to_string() })?;
                entry.set_password(api_key)
                    .map_err(|e| { log::error!("写入密钥链失败（详情：{}）", e); "保存 API 密钥失败，请检查系统密钥链权限".to_string() })?;
            }
        }
        
        let has_api_key_in_keyring = server.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false);
        
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO mcp_servers 
            (id, name, description, server_type, command, args, env, port, url, api_key, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            rusqlite::params![
                &server.id,
                &server.name,
                &server.description,
                &server_type,
                &server.command,
                &args_json,
                &env_json,
                &port,
                &server.url,
                if has_api_key_in_keyring { Some("__KEYRING__") } else { None },
                &server.enabled,
                &now,
                &now,
            ],
        )?;

        log::info!("MCP server saved: {} (API key stored in keyring)", server.id);
        Ok(())
    }

    fn get_mcp_api_key_from_keyring(server_id: &str) -> Option<String> {
        let entry = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")).ok()?;
        entry.get_password().ok()
    }

    pub fn get_mcp_servers(&self) -> Result<Vec<MCPServer>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, description, server_type, command, args, env, port, url, api_key, enabled, created_at, updated_at
            FROM mcp_servers 
            ORDER BY name ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let server_type_str: String = row.get(3)?;
            let server_type = match server_type_str.as_str() {
                "stdio" => MCPServerType::Stdio,
                "sse" => MCPServerType::SSE,
                "http" => MCPServerType::HTTP,
                _ => MCPServerType::Stdio,
            };
            
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let api_key_placeholder: Option<String> = row.get(9)?;
            
            let api_key = api_key_placeholder
                .filter(|k| k == "__KEYRING__")
                .and_then(|_| {
                    let id: Result<String, _> = row.get(0);
                    id.ok().and_then(|id_str| Self::get_mcp_api_key_from_keyring(&id_str))
                });
            
            Ok(MCPServer {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                server_type,
                command: row.get(4)?,
                args: serde_json::from_str(&args_json).unwrap_or_default(),
                env: serde_json::from_str(&env_json).unwrap_or_default(),
                port: row.get::<_, Option<i64>>(7)?.map(|p| p as u16),
                url: row.get(8)?,
                api_key,
                enabled: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;

        let servers: Result<Vec<_>, _> = rows.collect();
        Ok(servers?)
    }

    /**
     * 删除 MCP 服务器配置
     * 
     * @param server_id: 要删除的服务器 ID
     */
    pub fn delete_mcp_server(&self, server_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(entry) = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")) {
            let _ = entry.delete_credential();
        }
        
        self.conn.execute(
            "DELETE FROM mcp_servers WHERE id = ?1",
            [server_id],
        )?;

        log::info!("MCP server deleted: {} (including keyring entry)", server_id);
        Ok(())
    }

    /**
     * 保存 Skill 配置 (新建或更新)
     */
    pub fn save_skill(&self, skill: &Skill) -> Result<(), Box<dyn std::error::Error>> {
        let bound_servers_json = serde_json::to_string(&skill.bound_mcp_server_ids)?;
        let resource_files_json = serde_json::to_string(&skill.resource_files)?;

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO skills
            (id, name, description, instructions, bound_mcp_server_ids, enabled, resource_files, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            rusqlite::params![
                &skill.id,
                &skill.name,
                &skill.description,
                &skill.instructions,
                &bound_servers_json,
                &skill.enabled,
                &resource_files_json,
                &skill.created_at,
                &skill.updated_at,
            ],
        )?;

        log::info!("Skill saved: {}", skill.id);
        Ok(())
    }

    /**
     * 获取所有 Skill
     */
    pub fn get_skills(&self) -> Result<Vec<Skill>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, description, instructions, bound_mcp_server_ids, enabled, resource_files, created_at, updated_at
            FROM skills
            ORDER BY name ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let bound_servers_json: String = row.get(4)?;
            let resource_files_json: String = row.get(6)?;

            Ok(Skill {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                instructions: row.get(3)?,
                bound_mcp_server_ids: serde_json::from_str(&bound_servers_json).unwrap_or_default(),
                enabled: row.get(5)?,
                resource_files: serde_json::from_str(&resource_files_json).unwrap_or_default(),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let skills: Result<Vec<_>, _> = rows.collect();
        Ok(skills?)
    }

    /**
     * 删除 Skill
     */
    pub fn delete_skill(&self, skill_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "DELETE FROM skills WHERE id = ?1",
            [skill_id],
        )?;

        log::info!("Skill deleted: {}", skill_id);
        Ok(())
    }

    /**
     * 清空数据库：删除所有会话、消息、MCP 服务器配置、Skill。
     * 不涉及知识库 / 协作团队 / 定时任务，那些是各自独立的 SQLite 文件。
     */
    pub fn clear_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        // MCP 服务器的 API Key 存在系统密钥链里，delete 前先逐个清理，
        // 避免只删表行、密钥链里留下孤儿凭据。
        let mut stmt = self.conn.prepare("SELECT id FROM mcp_servers")?;
        let server_ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for server_id in &server_ids {
            if let Ok(entry) = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")) {
                let _ = entry.delete_credential();
            }
        }

        self.conn.execute("DELETE FROM messages", [])?;
        self.conn.execute("DELETE FROM sessions", [])?;
        self.conn.execute("DELETE FROM mcp_servers", [])?;
        self.conn.execute("DELETE FROM skills", [])?;
        self.conn.execute_batch("VACUUM")?;
        log::info!("Database cleared: all sessions, messages, mcp_servers, skills removed");
        Ok(())
    }
}

/// 数据库状态封装结构
/// 用于在 Tauri 应用中共享数据库实例
pub struct DbState(pub Arc<tokio::sync::Mutex<Database>>);
