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

use crate::commands::llm::{ChatMessage, ChatSession};
use crate::commands::mcp::{MCPServer, MCPServerType};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

/// 数据库实例包装结构
/// 包含数据库文件路径
pub struct Database {
    /// 数据库文件路径
    pub path: String,
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
        Self {
            path: db_path.to_str().unwrap().to_string(),
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
    pub async fn init(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        // Create sessions table
        conn.execute(
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

        // Add api_config_id column if it doesn't exist (for database migration)
        let has_column = conn.query_row(
            "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'api_config_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
        if !has_column {
            conn.execute(
                "ALTER TABLE sessions ADD COLUMN api_config_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            log::info!("Database migration: added api_config_id column");
        }

        // Create messages table
        conn.execute(
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

        // Create MCP servers table
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                server_type TEXT NOT NULL,
                command TEXT,
                args TEXT, -- JSON array of strings
                env TEXT, -- JSON object of key-value pairs
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

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled)",
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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            r#"
            INSERT OR REPLACE INTO sessions (id, title, provider, model, api_config_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            r#"
            INSERT OR REPLACE INTO messages (id, session_id, role, content, timestamp, error)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
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

        // Update session's updated_at
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            [&chrono::Utc::now().timestamp_millis().to_string(), session_id],
        )?;

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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, timestamp, error 
            FROM messages 
            WHERE session_id = ?1 
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
            })
        })?;

        let messages: Result<Vec<_>, _> = rows.collect();
        Ok(messages?)
    }

    /**
     * 保存 MCP 服务器配置
     * 
     * @param server: MCP 服务器配置
     */
    pub fn save_mcp_server(&self, server: &MCPServer) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
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

        conn.execute(
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
                &server.api_key,
                &server.enabled,
                &now,
                &now,
            ],
        )?;

        log::info!("MCP server saved: {}", server.id);
        Ok(())
    }

    /**
     * 获取所有 MCP 服务器配置
     * 按名称升序排列
     * 
     * @return MCP 服务器列表
     */
    pub fn get_mcp_servers(&self) -> Result<Vec<MCPServer>, Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
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
                _ => MCPServerType::Stdio, // default
            };
            
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            
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
                api_key: row.get(9)?,
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
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            "DELETE FROM mcp_servers WHERE id = ?1",
            [server_id],
        )?;

        log::info!("MCP server deleted: {}", server_id);
        Ok(())
    }
}

/// 数据库状态封装结构
/// 用于在 Tauri 应用中共享数据库实例
pub struct DbState(pub Arc<Mutex<Database>>);
